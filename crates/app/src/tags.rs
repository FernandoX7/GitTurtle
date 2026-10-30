//! Native tag browser, annotation inspector, and explicit captured actions.
use crate::*;
use focus_reveal::{FocusReveal, Trigger};
use gitturtle_core::{RemoteConfig, Tag, TagCommand, TagDetails, TagList, WriteCommand};
use gpui_kit::{
    component::{
        WindowExt,
        dialog::{DialogButtonProps, DialogFooter},
        scroll::{Scrollbar, ScrollbarHandle, ScrollbarMode},
    },
    prelude::FluentBuilder,
};

const PREPARING: &str = "Reading tags…";
/// The gap the kit's dialog leaves between its body and its footer, its
/// default 16 px padding. A dialog whose body keeps the focus ring's room
/// below its last control gives the room back by narrowing this gap.
pub(crate) const DIALOG_FOOTER_GAP: Pixels = px(16.);

/// The cue a scrolling list gives the rows beyond its edge, the same for
/// every list the app cues: the branch chooser, the remote manager, Tags,
/// the tag inspector's Push to… list, the Reflog's entries, the worktree
/// manager's lists, Stashes' detail, the GitHub panel's body, a document
/// side's own header controls and a 3D model's details. Where the
/// scrollbar's thumb can move along its track it is the always-visible
/// scrollbar the review dialogs cue theirs with; along a shorter track it
/// is a soft fade at each edge the rows continue past ([`edge_fades`]).
///
/// While the list overflows, its rows keep a gutter at their right, and
/// along a track at least [`SHORTEST_SCROLL_TRACK`] tall the scrollbar
/// paints its track there, over the column the rows lie in: inside the
/// list, clear of the room it keeps around its rows for their focus rings,
/// so the track covers no row and no ring, and clear of the dialog's side
/// padding, where the dialog's own scrollbar runs. The gutter is the
/// scrollbar's width plus the room of the ring every palette installs, so a
/// row's ring ends where the track begins, and neither the rows nor the
/// track move with the ring's room, which the list gives back. A list whose
/// rows fit keeps no gutter and draws no scrollbar, so its rows keep their
/// width.
///
/// Along a shorter track the kit's thumb ran out of its track against the
/// scroll or barely moved, as in a side header's controls region at its
/// 36–44 px floor, so there the list draws no scrollbar, leaving no
/// scrollbar element behind, and fades the edges its rows continue past
/// instead. It keeps the gutter, so nothing it holds moves as it grows or
/// shrinks past that length: without it, a document header's controls
/// wrapped wider and moved up a line when a click's answer shortened the
/// header past it. It still scrolls under a wheel and as Tab reveals a
/// control, and still reports itself, as every cued list does, as a scroll
/// view that clips its rows (`native_accessibility::clips_children`).
///
/// GPUI lays a frame out before it knows whether the list overflows, so the
/// gutter follows the overflow the list's scroll handle kept from the last
/// frame, and the scrollbar the length of the track it kept. A frame whose
/// layout differs draws once more after it is painted; a frame draws the
/// scrollbar only with the gutter, so a thumb never paints over a row. The
/// fades read the list as it is painted, so they follow its offset and its
/// reach in the frame that changes them.
pub(crate) struct ScrollCue {
    rows: RowColumn,
    /// Whether this frame keeps the gutter, and whether it draws the
    /// scrollbar in it ([`cues`]).
    cued: (bool, bool),
}

/// The shortest track a scrollbar's thumb moves along: the kit's shortest
/// thumb, an unscaled 48 px (`MIN_THUMB_SIZE` in `gpui-base`'s scrollbar),
/// and its 4 px inset at each of the track's ends (`THUMB_INSET`). Along a
/// shorter track the kit's thumb ran out of the track against the scroll,
/// or moved a few pixels at most, and a drag divided by the track's length
/// less the thumb's, at most zero.
pub(crate) const SHORTEST_SCROLL_TRACK: Pixels = px(48. + 4. * 2.);

/// How tall an edge fade is at the default interface size: about the room
/// a 28 px control keeps above and below its label, with its ring's, so a
/// fade over a control at the edge softens its ring and frame and leaves
/// its label clear. It scales with the interface text size, as the rows it
/// lies over do, and takes at most a quarter of the list, so at either size
/// the list's middle half reads unfaded.
const EDGE_FADE: f32 = 8.;

impl ScrollCue {
    /// The cue for the list `scroll` tracks, which keeps `room` around its
    /// rows for their rings, for the frame being rendered.
    pub(crate) fn new(scroll: &ScrollHandle, room: Pixels) -> Self {
        let rows = RowColumn {
            scroll: scroll.clone(),
            room,
        };
        Self {
            cued: cues(&rows),
            rows,
        }
    }

    /// For the list's `on_children_prepainted`: runs `prepainted`, then draws
    /// once more when this frame's layout cues otherwise than the cue
    /// assumed.
    pub(crate) fn observe(
        &self,
        prepainted: impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static,
    ) -> impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static {
        let (rows, cued) = (self.rows.clone(), self.cued);
        move |bounds, window, cx| {
            prepainted(bounds, window, cx);
            if cues(&rows) != cued {
                window.defer(cx, |window, _| window.refresh());
            }
        }
    }

    /// `list`, with the gutter while it overflows, the scrollbar `id` in it
    /// along a track a thumb moves on, and otherwise the fades at the edges
    /// its rows continue past. The list is a node that clips its rows
    /// (`native_accessibility::clips_children`), so a row scrolled out of
    /// view is out of reach as well as out of sight. Tests find the
    /// scrollbar, while it is drawn, by `id`.
    pub(crate) fn list(&self, list: Stateful<Div>, id: &'static str) -> Div {
        let ring = appearance::BUTTON_FOCUS_RING;
        let gutter = Scrollbar::width() + ring.gap + ring.width;
        let room = self.rows.room;
        let (kept, scrollbar) = self.cued;
        let cue = div()
            .flex()
            .flex_col()
            .child(crate::native_accessibility::clips_children(
                list.when(kept, |list| list.pr(room + gutter)),
            ));
        if scrollbar {
            cue.child(
                div()
                    .debug_selector(move || id.into())
                    .absolute()
                    .size_full()
                    .child(
                        Scrollbar::vertical(&self.rows)
                            .id(id)
                            .mode(ScrollbarMode::Always),
                    ),
            )
        } else {
            cue.child(edge_fades(self.rows.scroll.clone()))
        }
    }
}

/// The fades over a list too short for its scrollbar's thumb to move
/// ([`ScrollCue`]): at each edge its rows continue past, a band as wide as
/// the list and [`EDGE_FADE`] tall, opaque at the edge in the theme's
/// background colour (the palette's canvas, which panes and dialogs are
/// drawn on) and clear inward, so the rows there dissolve into the surface
/// rather than end at a cut, as macOS fades content scrolled beneath a
/// window's edge. It draws none at an end the list is scrolled to, and
/// none on a list whose rows fit.
///
/// It reads the list's bounds, offset and reach as it paints, after the
/// list has laid out and clamped its offset in the same frame, so a wheel,
/// a reveal, a resize or rows that arrive show their fades in the first
/// frame that draws them; read as the cue renders, rows that arrived drew
/// no fade until something else drew the list again. It inserts no hitbox,
/// so the pointer reaches what lies beneath it: a wheel over a fade scrolls
/// the list, and a row under one takes its hover and click. It does not
/// move: a fade comes and goes with the scroll that reveals an edge, in
/// that frame.
fn edge_fades(scroll: ScrollHandle) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _, window, cx| {
            let list = scroll.bounds();
            let fade = appearance::ui_size(EDGE_FADE).min(list.size.height / 4.);
            if fade <= Pixels::ZERO {
                return;
            }
            // Half a pixel either way is an end: GPUI rounds the reach to
            // two decimals.
            let slack = px(0.5);
            let scrolled = -scroll.offset().y;
            let surface: Hsla = rgb(appearance::palette(cx).canvas).into();
            let band = |angle: f32| {
                linear_gradient(
                    angle,
                    linear_color_stop(surface.opacity(0.), 0.),
                    linear_color_stop(surface, 1.),
                )
            };
            if scrolled > slack {
                // Toward the top, opaque at the list's top edge.
                let top = Bounds::new(list.origin, size(list.size.width, fade));
                window.paint_quad(fill(top, band(0.)));
            }
            if scrolled < scroll.max_offset().y - slack {
                // Toward the bottom, opaque at the list's bottom edge.
                let bottom = Bounds::new(
                    point(list.left(), list.bottom() - fade),
                    size(list.size.width, fade),
                );
                window.paint_quad(fill(bottom, band(180.)));
            }
        },
    )
    .absolute()
    .size_full()
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

/// Whether the list `rows` reads keeps its gutter, where it overflowed when
/// last laid out, and whether it draws its scrollbar there, along a track a
/// thumb moves on ([`SHORTEST_SCROLL_TRACK`]). The kit lays that track over
/// the scrollbar's viewport, the list less the room it keeps around its
/// rows (`RowColumn::viewport_bounds`); judged by the list's whole height,
/// a list keeping room drew a thumb along a track up to twice that room
/// shorter than the shortest.
fn cues(rows: &RowColumn) -> (bool, bool) {
    let overflows = overflows(&rows.scroll);
    (
        overflows,
        overflows && rows.viewport_bounds().size.height >= SHORTEST_SCROLL_TRACK,
    )
}

use crate::native_accessibility::text as static_text;

#[derive(Default)]
pub(super) struct State {
    generation: u64,
    task: Option<Task<()>>,
    draft: Option<Entity<TagForm>>,
    /// The Tags list or Push to… list opened last, for tests.
    #[cfg(test)]
    list: Option<FocusReveal>,
    /// The filter of the Tags dialog opened last, for tests.
    #[cfg(test)]
    filter: Option<Entity<InputState>>,
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
                    // An unborn HEAD with no commit in History: nothing for a
                    // tag to name.
                    let committed = !(this.commits.is_empty()
                        && this
                            .work_status
                            .as_ref()
                            .is_some_and(|status| status.head.is_none()));
                    let browser =
                        cx.new(|cx| TagBrowser::new(owner, path, list, committed, window, cx));
                    #[cfg(test)]
                    {
                        this.tag_actions.list = Some(browser.read(cx).rows.clone());
                    }
                    let filter = browser.read(cx).query.read(cx).focus_handle(cx);
                    #[cfg(test)]
                    {
                        this.tag_actions.filter = Some(browser.read(cx).query.clone());
                    }
                    window.open_alert_dialog(cx, move |dialog, window, cx| {
                        // The dialog clips its body to the body's bounds, and
                        // the browser keeps the focus ring's room inside them
                        // above Create tag… and below the last row. The
                        // title's margin and the footer's gap give the room
                        // back, so nothing in the dialog moves.
                        let room = appearance::button_ring_room(cx);
                        dialog
                            .title(
                                native_accessibility::dialog_title("tags-dialog-title", "Tags")
                                    .ml(TEXT_INSET)
                                    .mb(-room),
                            )
                            .width(appearance::dialog_width(px(600.), window))
                            .max_h(appearance::dialog_max_height(window))
                            .gap(DIALOG_FOOTER_GAP - room)
                            .child(browser.clone())
                            .button_props(DialogButtonProps::default().ok_text("Done"))
                    });
                    // Opening the dialog focuses its shell, which draws no
                    // ring: the filter, the first field, takes focus so the
                    // first frame draws its ring, as the Reflog's does.
                    filter.focus(window, cx);
                    window.refresh();
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
                this.read_deferred_working(window, cx);
                if this.path.as_ref() == Some(&path) && this.page == AppPage::Repository {
                    receive(result, this, window, cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
        true
    }
    pub(crate) fn inspect_tag(&mut self, tag: Tag, window: &mut Window, cx: &mut Context<Self>) {
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
            let tag_mark = copy_feedback::Mark::new("copy-tag-oid", &tag.oid);
            let tag_copy = tag_mark.clone();
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
            // A write refuses Delete and Push at once, and assistive
            // technology hears them unavailable at once, while they dim only
            // once it looks busy (`workspace::BusyLook`); each refused a
            // press silently, announced and drawn available. This dialog is
            // built in the window's render, so each reads the window's
            // state as it lays out (`workspace::dialog_refusing`).
            let content = div().flex().flex_col().gap_3().pb(room)
                .child(static_text("tag-target-identity", format!("{} · {} {}", if tag.annotated { "Annotated tag" } else { "Lightweight tag" }, tag.target_kind, tag.target_oid)).text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)))
                .child(static_text("tag-object-identity", format!("Tag object: {}", tag.oid)).text_size(crate::appearance::ui_text(11.)))
                .when(!tag.tagger.is_empty(), |element| element.child(static_text("tag-author", format!("Tagged by {}", tag.tagger)).text_size(crate::appearance::ui_text(12.))))
                .when_some(annotation.as_ref(), |element, annotation| element.child(crate::editor_find::Editor::new(annotation).readonly(true).h(px(160.)).aria_label("Tag annotation and signature, if present")))
                .when_some(details.annotation_unavailable.as_ref(), |element, message| element.child(static_text("tag-annotation-unavailable", message.clone()).text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.warning))))
                .child(div().flex().gap_2()
                    .child(copy_feedback::copy_button(tag_mark, "Copy object ID", "", cx).on_click(move |_, window, cx| copy_feedback::copy(window, cx, Some(tag_copy.clone()), "tag object ID", oid.clone())))
                    .child(crate::workspace::dialog_refusing(owner.clone(), path.clone(), move |refused, dimmed| button("delete-local-tag", "Delete local tag…", "", false).debug_selector(|| "delete-local-tag".into()).map(|delete| crate::workspace::refusing(delete, || "delete-local-tag".into(), refused, dimmed)).on_click(move |_, window, cx| {
                        let _ = delete_owner.update(cx, |this, cx| {
                            if !this.dialog_refusal(delete_path.as_deref()).0 {
                                window.close_dialog(cx);
                                this.confirm_git_write(format!("Delete local tag '{}'", deletion.name), format!("Remove the local name '{}' pointing to {}. Its annotation and signature will no longer be reachable through this name.\n\nRemote tags, branches, the index and working files stay as they are. This action refuses if the tag has moved since you opened it.", deletion.name, deletion.oid), "Delete local tag", WriteCommand::Tag(Arc::new(TagCommand::Delete(deletion.clone()))), window, cx);
                            }
                        });
                    }).into_any_element())))
                .child(static_text("tag-push-heading", "Push this tag").text_size(crate::appearance::ui_text(12.)).font_weight(FontWeight::MEDIUM))
                .child(static_text("tag-push-consequences", "Choose one remote, then review. No other tags or branches are pushed. Existing remote tags are never replaced.").text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)))
                .child(cue.list(div().on_children_prepainted(cue.observe(reveal)).id("tag-remote-list").debug_selector(|| "tag-remote-list".into()).max_h(px(160.) + room * 2.).p(room).m(-room).overflow_y_scroll().track_scroll(list.scroll()).flex().flex_col().gap_1().children(remotes.iter().zip(pushes).enumerate().map(|(index, (remote, focus))| {
                    let remote = remote.clone(); let tag = tag.clone(); let owner = owner.clone(); let path = path.clone();
                    crate::workspace::dialog_refusing(owner.clone(), path.clone(), move |refused, dimmed| button(("push-tag-remote", index), format!("Push to {}…", remote.name), "", false).track_focus(&focus).debug_selector(move || format!("push-tag-remote-{index}")).map(|push| crate::workspace::refusing(push, || format!("push-tag-remote-{index}"), refused, dimmed)).on_click(move |_, window, cx| {
                        let _ = owner.update(cx, |this, cx| {
                            if !this.dialog_refusal(path.as_deref()).0 {
                                window.close_dialog(cx);
                                let urls = if remote.push_urls.is_empty() { &remote.urls } else { &remote.push_urls };
                                let destinations = urls.iter().map(|url| crate::workspace::display_remote_url(url)).collect::<Vec<_>>().join("\n");
                                this.confirm_git_write(format!("Push tag '{}'", tag.name), format!("Send only '{}' ({}) to remote '{}':\n{}\n\nThis explicit network action creates the named remote tag. It does not push branches, additional tags, or overwrite an existing remote tag.", tag.name, tag.oid, remote.name, destinations), "Push named tag", WriteCommand::Tag(Arc::new(TagCommand::Push { tag: tag.clone(), remote: Arc::new(remote.clone()) })), window, cx);
                            }
                        });
                    }).into_any_element())
                })).when(remotes.is_empty(), |element| element.child(static_text("tag-remotes-empty", "No remotes configured. Add a remote from the branch menu to push a tag.").text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)))), "tag-remote-scrollbar"));
            dialog.title(native_accessibility::dialog_title("tag-inspector-title", details.tag.name.clone())).width(appearance::dialog_width(px(640.), window)).max_h(appearance::dialog_max_height(window)).gap(DIALOG_FOOTER_GAP - room).child(content).button_props(DialogButtonProps::default().ok_text("Done"))
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
        window.open_alert_dialog(cx, move |dialog, window, _| {
            let submit = form.clone();
            let cancel = form.clone();
            dialog
                .title(native_accessibility::dialog_title(
                    "create-tag-dialog-title",
                    "Create a local tag",
                ))
                .width(appearance::dialog_width(px(560.), window))
                .max_h(appearance::dialog_max_height(window))
                .child(form.clone())
                // The kit's own Cancel and OK, built here rather than by the
                // kit so that Review tag can follow a write (`ReviewTag`).
                .footer(
                    DialogFooter::new()
                        .child({
                            // Cancels its own form and closes the dialog,
                            // wherever focus is: asking the focused element
                            // to cancel missed the dialog once Settings had
                            // shown and taken focus.
                            let form = form.clone();
                            Button::new("cancel")
                                .label("Cancel")
                                .debug_selector(|| "cancel-create-tag".into())
                                .on_click(move |_, window, cx| {
                                    form.update(cx, |form, cx| form.cancel(window, cx));
                                    window.close_dialog(cx);
                                })
                        })
                        .child(ReviewTag { form: form.clone() }),
                )
                .on_ok(move |_, window, cx| {
                    submit.update(cx, |this, cx| this.submit(window, cx));
                    false
                })
                .on_cancel(move |_, window, cx| {
                    cancel.update(cx, |form, cx| form.cancel(window, cx));
                    true
                })
        });
    }
}

struct TagBrowser {
    owner: WeakEntity<GitTurtle>,
    path: Option<PathBuf>,
    list: TagList,
    /// Whether the repository has a commit for a tag to name. With none,
    /// Create tag… cannot work: it is disabled with its reason, and the
    /// empty list points at the first commit.
    committed: bool,
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
        committed: bool,
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
            committed,
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
    /// Inspects `tag`, as its row or Enter in the filter asks, unless the
    /// window refuses the dialog's actions, as the rows are announced
    /// (`workspace::dialog_refusal`): a write runs, or Tags' repository or
    /// the Repository page is no longer shown. It once checked only the
    /// repository and the write, so a press off the Repository page closed
    /// Tags for a read that then refused.
    fn activate(&self, tag: Tag, window: &mut Window, cx: &mut Context<Self>) {
        let _ = self.owner.update(cx, |this, cx| {
            if !this.dialog_refusal(self.path.as_deref()).0 {
                window.close_dialog(cx);
                this.inspect_tag(tag, window, cx);
            }
        });
    }
}
/// Tags' line in a repository with no commits, where a tag has nothing to
/// name.
const NO_COMMIT_LINE: &str = "No commits to tag yet · Your first commit starts in Changes";
/// Create tag…'s name and tooltip there, which say why it is unavailable.
const NO_COMMIT_NAME: &str = "Create tag…, unavailable until the first commit";
const NO_COMMIT_REASON: &str = "A tag names a commit · Your first commit starts in Changes";

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
        //
        // The dialog's text starts at one inset, the filter's: the title,
        // the summary, the notices, the empty list's line and each row's
        // name, which the kit centred in its row while the summary started
        // at the dialog's edge. With no tag at all the line alone says so;
        // the summary above it offered to inspect, delete or push a tag
        // that was not there.
        let room = appearance::button_ring_room(cx);
        let rows: Vec<_> = (0..matches.len().min(100))
            .map(|index| self.rows.focus(("tag-row", index), cx))
            .collect();
        let reveal = self.rows.items(rows.clone(), room);
        // Past its height the list cues the rest with its scrollbar.
        let cue = ScrollCue::new(self.rows.scroll(), room);
        // A write refuses Create tag… and the rows, which open a tag, at
        // once, and assistive technology hears them unavailable at once,
        // while they dim only once it looks busy (`workspace::BusyLook`);
        // each refused a press silently, announced and drawn available.
        let (refused, dimmed) =
            crate::workspace::dialog_refusal(&self.owner, self.path.as_deref(), cx);
        let committed = self.committed;
        div().flex().flex_col().gap_3().py(room)
            // Painted after the field, the canvas fits its placeholder in
            // whole words, where at 18 pt in a half tile it was cut mid-glyph.
            .child(div().flex().gap_2().child(div().relative().flex_1().child(Input::new(&self.query).aria_label("Filter local tags").cleanable(true)).child(crate::workspace::placeholder_fit(self.query.clone(), &FILTER_FIT, FILTER_PLACEHOLDERS, FIELD_TEXT))).child(button("create-tag", "Create tag…", "plus", false).debug_selector(|| "create-tag".into()).when(!committed, |create| create.accessibility_label(NO_COMMIT_NAME).tooltip(NO_COMMIT_REASON)).map(|create| crate::workspace::refusing(create, || "create-tag".into(), refused || !committed, dimmed || !committed)).on_click(cx.listener(|this, _, window, cx| {
                let _ = this.owner.update(cx, |owner, cx| { if !owner.dialog_refusal(this.path.as_deref()).0 { window.close_dialog(cx); owner.open_create_tag(window, cx); } });
            }))))
            .when(!self.list.tags.is_empty(), |element| element.child(static_text("tag-list-summary", summary(self.list.tags.len())).ml(TEXT_INSET).text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted))))
            .child(cue.list(div().on_children_prepainted(cue.observe(reveal)).id("tags-list").debug_selector(|| "tags-list".into()).max_h(px(360.) + room * 2.).p(room).m(-room).overflow_y_scroll().track_scroll(self.rows.scroll()).flex().flex_col().gap_1().children(matches.iter().zip(rows).enumerate().map(|(index, (tag, focus))| {
                let tag = (*tag).clone(); let label = format!("{} · {} · {}", tag.name, if tag.annotated { "Annotated" } else { "Lightweight" }, short_oid(&tag.target_oid));
                Button::new(("tag-row", index)).track_focus(&focus).debug_selector(move || format!("tag-row-{index}")).ghost().w_full().h(crate::appearance::ui_size(34.)).pl(TEXT_INSET).child(row_text(format!("tag-row-text-{index}"), label.clone())).accessibility_label(label).map(|row| crate::workspace::refusing(row, || format!("tag-row-{index}"), refused, dimmed)).on_click(cx.listener(move |this, _, window, cx| this.activate(tag.clone(), window, cx)))
            })).when(matches.is_empty(), |element| element.child(div().py_3().ml(TEXT_INSET).child(EmptyState::new("tag-list-empty", if !self.committed { NO_COMMIT_LINE } else if self.list.tags.is_empty() { "No local tags yet · Create tag… names a commit" } else { "No tags match this filter" }).block(cx)))), "tags-scrollbar"))
            .when(matches.len() > 100, |element| element.child(static_text("tag-list-match-limit", "Showing 100 matches. Narrow the filter to find another tag.").ml(TEXT_INSET).text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted))))
            .when(self.list.truncated, |element| element.child(static_text("tag-list-load-limit", "Loaded the first 10,000 local tags by name. Additional tags are outside this bounded browser.").ml(TEXT_INSET).text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.warning))))
    }
}

/// The summary above a list of `count` tags, the count agreeing with its
/// noun: "1 local tag" read "1 local tags".
fn summary(count: usize) -> String {
    format!(
        "{} · Select to inspect, delete locally, or push one named tag.",
        crate::counted(count, "local tag", "local tags")
    )
}

/// Where the Tags dialog's text starts, in from its content edge: the
/// filter's text inset, where its placeholder starts at every size. The
/// title, the summary, the notices, the empty list's line and each row's
/// name start here, as the Reflog's text does.
const TEXT_INSET: Pixels = appearance::FIELD_TEXT_INSET;

/// The filter's placeholders, each a shorter wording of the one before it in
/// whole words: the first that fits the field shows
/// (`workspace::placeholder_fit`). The field keeps its accessible name,
/// "Filter local tags", whichever shows.
const FILTER_PLACEHOLDERS: &[&str] =
    &["Filter tags by name or object ID", "Filter tags…", "Filter"];

/// The size the kit's medium field draws its text at, at the default
/// interface size: 0.875 rem of a 13 px rem.
const FIELD_TEXT: f32 = 13. * 0.875;

thread_local! {
    /// The filter's placeholder as it fits the width the field last painted at.
    static FILTER_FIT: crate::workspace::PlaceholderFit = const { std::cell::RefCell::new(None) };
}

/// A list row's name, as a list row starts it rather than a centred Button
/// label: from the row's leading inset, on one line ending in an ellipsis
/// where the row is too narrow. The row's accessible name keeps it whole.
pub(crate) fn row_text(selector: String, text: String) -> Div {
    div()
        .debug_selector(move || selector.clone())
        .flex_1()
        .min_w_0()
        .whitespace_nowrap()
        .text_ellipsis()
        .line_height(relative(1.))
        .child(text)
}

struct TagForm {
    owner: WeakEntity<GitTurtle>,
    path: Option<PathBuf>,
    name: Entity<InputState>,
    target: Entity<InputState>,
    message: Entity<TextareaState>,
    annotated: bool,
    error: Option<String>,
    /// The generation of the window's tag read this form started
    /// (`TagForm::pending`).
    preparing: Option<u64>,
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
            preparing: None,
        }
    }
    /// Whether the read this form started still runs: the window's tag read
    /// of its generation, which ends when it lands, is dropped, or is
    /// cancelled. A flag the read's reply cleared stayed set when the reply
    /// was dropped, as one landing while Settings showed was, so every
    /// later Review tag was refused while it looked available.
    fn pending(&self, cx: &App) -> bool {
        self.preparing.is_some_and(|generation| {
            self.owner.upgrade().is_some_and(|owner| {
                let owner = owner.read(cx);
                owner.tag_actions.generation == generation
                    && owner.operation_busy == Some(PREPARING)
            })
        })
    }
    /// Ends the form's preparation and its read, as Cancel or Escape closes
    /// the dialog, and runs the working-state read asked for meanwhile,
    /// which the cancelled preparation's end would have run.
    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.preparing = None;
        let _ = self.owner.update(cx, |this, cx| {
            this.cancel_tag_action();
            this.read_deferred_working(window, cx);
        });
    }
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Refused at once while a write runs, as Review tag is announced
        // (`ReviewTag`), before anything changes: a press set the form's
        // error, "The repository changed or another operation is running".
        // The form's own read runs as such a busy spell, so it is refused
        // here too.
        if crate::workspace::dialog_refusal(&self.owner, self.path.as_deref(), cx).0 {
            return;
        }
        let name = self.name.read(cx).value().trim().to_owned();
        let target = self.target.read(cx).value().trim().to_owned();
        let message = self
            .annotated
            .then(|| self.message.read(cx).value().to_string());
        let form = cx.entity().downgrade();
        let preparing = self.owner.update(cx, |this, cx| {
            if this.path != self.path { return None; }
            this.read_tag_action(move |repo| repo.create_tag_plan(&name, &target, message), move |result, this, window, cx| {
                let _ = form.update(cx, |form, cx| { form.preparing = None; cx.notify(); });
                match result {
                    Ok(plan) => {
                        window.close_dialog(cx);
                        let explanation = format!("Create local tag '{}' at commit {}.\n\n{}{}\n\n{}\n\nNo remote is contacted. Your branches, index, and working files stay as they are.", plan.name, plan.target_oid, if plan.annotation.is_some() { "Annotated tag." } else { "Lightweight tag." }, if plan.signing { " Git's configured signing is enabled." } else { " Git's configured signing behavior is preserved." }, plan.annotation.as_deref().unwrap_or("This tag has no annotation."));
                        this.confirm_git_write("Create local tag".into(), explanation, "Create tag", WriteCommand::Tag(Arc::new(TagCommand::Create(plan))), window, cx);
                    }
                    Err(error) => { let _ = form.update(cx, |form, cx| { form.error = Some(format!("{error:#}")); cx.notify(); }); }
                }
            }, window, cx).then_some(this.tag_actions.generation)
        }).ok().flatten();
        self.preparing = preparing;
        if preparing.is_none() {
            self.error = Some("The repository changed or another operation is running. Reopen Create tag when it finishes.".into());
        }
        cx.notify();
    }
}
impl Render for TagForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let pending = self.pending(cx);
        div().flex().flex_col().gap_3()
            .child(static_text("tag-name-label", "Tag name").text_size(crate::appearance::ui_text(12.))).child(Input::new(&self.name).aria_label("Tag name"))
            .child(static_text("tag-target-label", "Target commit (selected commit by default)").text_size(crate::appearance::ui_text(12.))).child(Input::new(&self.target).aria_label("Target commit for tag"))
            .child(div().flex().gap_2().children([(false, "Lightweight"), (true, "Annotated")].map(|(annotated, label)| button(label, label, "", self.annotated == annotated).toggled(self.annotated == annotated).disabled(pending).on_click(cx.listener(move |this, _, _, cx| { this.annotated = annotated; this.error = None; cx.notify(); })))))
            .when(self.annotated, |element| element.child(shortcuts::tab_leaves(Textarea::new(&self.message).aria_label("Tag annotation"))))
            .child(static_text("tag-signing-explanation", "Signing follows Git configuration. Signing failures are reported without an unsigned fallback.").text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)))
            .when(pending, |element| element.child(static_text("tag-preparation-status", "Preparing exact target and signing settings…").text_size(crate::appearance::ui_text(12.))))
            .children(self.error.as_ref().map(|error| static_text("tag-preparation-error", error.clone()).text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.warning))))
    }
}

/// Review tag's accessible name while the window's busy spell refuses it:
/// unavailable, and why, in the words of the `reason` its tooltip gives
/// once it dims (`GitTurtle::busy_reason`), as Create tag…'s name and
/// tooltip say why it waits for a first commit. Its name and tooltip once
/// named the current Git operation whatever the spell ran.
fn review_tag_busy_name(reason: &str) -> String {
    format!("Review tag, unavailable. {reason}")
}

/// Create tag's Review tag, the kit's OK as its own footer would draw it.
/// A write refuses it at once, and assistive technology hears it unavailable
/// at once, its name saying why, while it dims, its tooltip saying why, only
/// once the write looks busy (`workspace::BusyLook`), as Tags' own actions
/// do (`workspace::dialog_refusal`). The kit drew it available through any
/// write, and a press set the form's error. The dialog is built in the
/// window's render, so Review tag reads the window's state as it lays out,
/// as `workspace::dialog_refusing` does. The form's own preparation runs as
/// a read the window counts busy; its status line says what it does, so it
/// gives no reason of its own.
#[derive(IntoElement)]
struct ReviewTag {
    form: Entity<TagForm>,
}

impl RenderOnce for ReviewTag {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        review_tag(self.form, cx)
    }
}

/// Review tag as `form` and the window leave it now ([`ReviewTag`]).
fn review_tag(form: Entity<TagForm>, cx: &App) -> Button {
    let state = form.read(cx);
    // The form's own read runs as a busy spell, which refuses Review tag and
    // dims it with its busy look; it gives no reason, as its status line
    // says what it does.
    let pending = state.pending(cx);
    // Why the window's busy spell refuses it, while one other than the
    // form's own runs: the spell's own reason, which names a GitHub
    // request's work where one runs (`GitTurtle::busy_reason`).
    let (refused, dimmed, reason) = state.owner.upgrade().map_or((true, true, None), |owner| {
        let owner = owner.read(cx);
        let (refused, dimmed) = owner.dialog_refusal(state.path.as_deref());
        let busy = owner.operation_busy.is_some() && !pending;
        (refused, dimmed, busy.then(|| owner.busy_reason()))
    });
    let tooltip = reason.filter(|_| dimmed);
    #[cfg(test)]
    REVIEW_TAG.with(|review| review.set(tooltip));
    {
        let review = Button::new("ok")
            .label("Review tag")
            .primary()
            .debug_selector(|| "review-tag".into())
            .when_some(reason, |review, reason| {
                review.accessibility_label(review_tag_busy_name(reason))
            })
            .when_some(tooltip, |review, reason| review.tooltip(reason));
        // A press submits its own form, wherever focus is: the kit's OK
        // asked the focused element to confirm, which missed the dialog
        // once Settings had shown and taken focus.
        crate::workspace::refusing(review, || "review-tag".into(), refused, dimmed)
            .on_click(move |_, window, cx| form.update(cx, |form, cx| form.submit(window, cx)))
    }
}

#[cfg(test)]
thread_local! {
    /// The tooltip Review tag was last built with, which no node carries.
    static REVIEW_TAG: std::cell::Cell<Option<&'static str>> =
        const { std::cell::Cell::new(None) };
}

/// The filter of the Tags dialog opened last, for the focus sweep.
#[cfg(test)]
pub(crate) fn filter(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> FocusHandle {
    app.read_with(cx, |app, cx| {
        app.tag_actions
            .filter
            .as_ref()
            .map(|query| query.read(cx).focus_handle(cx))
    })
    .expect("Tags opened")
}

/// Opens Create tag, annotated as it opens, and returns its annotation
/// field, for the prose fields' Tab regression in `views::tests`.
#[cfg(test)]
pub(crate) fn open_tag_annotation(
    app: &mut GitTurtle,
    window: &mut Window,
    cx: &mut Context<GitTurtle>,
) -> Entity<TextareaState> {
    app.open_create_tag(window, cx);
    let form = app.tag_actions.draft.clone().expect("Create tag opens");
    let form = form.read(cx);
    assert!(form.annotated, "Create tag opens annotated");
    form.message.clone()
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
        let repo = crate::views::tests::fixture_repository(directory.join("repository"));
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
        crate::views::tests::init_app(cx);
        cx.update(|cx| {
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

    /// Returns once the read `open_tags` or `inspect_tag` started has landed.
    /// It runs on the app's operation executor, a thread `run_until_parked`
    /// cannot wait for, and the dialog it prepares opens only with its reply,
    /// so a wait in wall-clock time missed a slow read.
    pub(crate) async fn read_lands(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        let read = app.update(cx, |app, _| app.tag_actions.task.take());
        read.expect("a tag read was started").await;
    }

    /// The tooltips hovering `selector` newly paints, by the kit's popover
    /// fill; the pointer then leaves the window until they have gone.
    pub(crate) fn hovered_tooltips(cx: &mut VisualTestContext, selector: &'static str) -> usize {
        const SHOW_DELAY: std::time::Duration = std::time::Duration::from_millis(500);
        let popover = cx.read(|cx| {
            gpui_kit::Background::from(gpui_kit::component::Theme::global(cx).tokens.popover)
        });
        let popups = |cx: &mut VisualTestContext| {
            cx.update(|window, _| {
                window
                    .painted_quads()
                    .into_iter()
                    .filter(|quad| quad.background == popover)
                    .map(|quad| quad.bounds)
                    .collect::<Vec<_>>()
            })
        };
        let before = popups(cx);
        let control = rendered(cx, selector);
        cx.simulate_mouse_move(control.center(), None, Modifiers::default());
        cx.executor().advance_clock(SHOW_DELAY);
        draw(cx);
        let shown = popups(cx)
            .into_iter()
            .filter(|popup| !before.contains(popup))
            .count();
        cx.simulate_mouse_move(point(px(-1.), px(-1.)), None, Modifiers::default());
        cx.executor().advance_clock(SHOW_DELAY);
        draw(cx);
        shown
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
    pub(crate) fn painted_thumb(
        cx: &mut VisualTestContext,
        track: Bounds<Pixels>,
    ) -> Option<Bounds<Pixels>> {
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
        read_lands(&app, cx).await;
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
        read_lands(&app, cx).await;
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

    /// Opens Settings, as its shortcut does over an open dialog, and
    /// presses `control` where the last frame drew it before a frame shows
    /// Settings: the press reaches the handler the Repository page's frame
    /// gave the control, which the next frame dims.
    fn press_as_settings_open(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        control: &'static str,
    ) {
        let position = rendered(cx, control).center();
        cx.update(|window, cx| {
            app.update(cx, |app, cx| app.show_settings(window, cx));
            let button = gpui::MouseButton::Left;
            let modifiers = Modifiers::default();
            window.dispatch_event(
                gpui::PlatformInput::MouseDown(gpui::MouseDownEvent {
                    button,
                    position,
                    modifiers,
                    click_count: 1,
                    first_mouse: false,
                }),
                cx,
            );
            window.dispatch_event(
                gpui::PlatformInput::MouseUp(gpui::MouseUpEvent {
                    button,
                    position,
                    modifiers,
                    click_count: 1,
                }),
                cx,
            );
        });
        draw(cx);
    }

    /// Off the Repository page Tags' actions are refused, as each is
    /// announced (`workspace::dialog_refusal`), and their handlers refuse
    /// with them: a tag's row pressed as Settings opens leaves Tags open and
    /// reads nothing, and Delete local tag… leaves the tag's inspector open
    /// and asks for no review. The handlers checked only the repository and
    /// the write, so the row closed Tags for a read the page then refused,
    /// and Delete closed the inspector and asked to review deleting the tag
    /// over Settings.
    #[gpui::test]
    async fn tag_actions_refuse_off_the_repository_page(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        let dialog_open =
            |cx: &mut VisualTestContext| cx.update(|window, cx| window.has_active_dialog(cx));
        let refused = |control: &str| crate::workspace::refusals::get(control);
        let off_page = Some(crate::workspace::refusals::Refusal {
            announced: true,
            dimmed: true,
        });

        cx.update(|window, cx| app.update(cx, |app, cx| app.open_tags(window, cx)));
        read_lands(&app, cx).await;
        draw(cx);
        press_as_settings_open(&app, cx, "tag-row-0");
        assert!(dialog_open(cx), "the row leaves Tags open");
        assert_eq!(refused("tag-row-0"), off_page, "the row is refused");
        assert!(
            app.read_with(cx, |app, _| app.tag_actions.task.is_none()
                && app.operation_busy.is_none()),
            "the row reads nothing"
        );
        cx.update(|window, cx| {
            window.close_dialog(cx);
            app.update(cx, |app, cx| app.show_history(window, cx));
        });
        draw(cx);

        let tag = repo.tags().unwrap().tags[0].clone();
        cx.update(|window, cx| app.update(cx, |app, cx| app.inspect_tag(tag, window, cx)));
        read_lands(&app, cx).await;
        draw(cx);
        press_as_settings_open(&app, cx, "delete-local-tag");
        assert!(
            cx.debug_bounds("operation-consequences").is_none(),
            "Delete asks for no review"
        );
        assert!(
            cx.debug_bounds("tag-inspector-title").is_some(),
            "Delete leaves the inspector open"
        );
        assert_eq!(
            refused("delete-local-tag"),
            off_page,
            "Delete local tag… is refused"
        );
    }

    /// Create tag opened over a tagged repository in a 1480 × 980 window,
    /// naming a lightweight tag, which needs no annotation: the window and
    /// its form.
    fn create_lightweight_tag<'a>(
        cx: &'a mut TestAppContext,
        fixture: &std::path::Path,
    ) -> (
        Entity<GitTurtle>,
        Entity<TagForm>,
        &'a mut VisualTestContext,
    ) {
        let repo = tagged_repository(fixture);
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(1480.), px(980.)));
        draw(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_create_tag(window, cx)));
        draw(cx);
        let form = app
            .read_with(cx, |app, _| app.tag_actions.draft.clone())
            .expect("Create tag opens");
        cx.update(|window, cx| {
            form.update(cx, |form, cx| {
                form.annotated = false;
                form.name
                    .update(cx, |name, cx| name.set_value("v9.0", window, cx))
            })
        });
        draw(cx);
        (app, form, cx)
    }

    /// Review tag's accessibility node as the window would write it now,
    /// built inside a draw as `views::tests::input_node` builds a field's,
    /// and the tooltip the next frame builds it with, which no node carries.
    fn review_tag_frame(
        form: &Entity<TagForm>,
        cx: &mut VisualTestContext,
    ) -> (gpui::accesskit::Node, Option<&'static str>) {
        let form = form.clone();
        let node = crate::views::tests::view_node(cx, move |window, cx| {
            use gpui::Element as _;
            let button = review_tag(form.clone(), cx).base(window, cx);
            let element = RenderOnce::render(button, window, cx).into_element();
            let mut node =
                gpui::accesskit::Node::new(element.a11y_role().expect("Review tag's role"));
            element.write_a11y_info(&mut node);
            node
        });
        // What the window's frame records, as the node's build did too.
        crate::workspace::refusals::forget();
        REVIEW_TAG.with(|review| review.set(None));
        cx.update(|window, cx| {
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        });
        (node, REVIEW_TAG.with(|review| review.get()))
    }

    /// Create tag's Review tag follows a write as Tags' own actions do. A
    /// write that ends within the busy look's delay leaves it looking
    /// available in every frame, announced unavailable, by a name that says
    /// why, only while the write runs. A slower write refuses a press at
    /// once, before anything changes, and dims it, its tooltip saying why,
    /// once the write has run 200 ms; when the write ends it is available
    /// again and Enter opens the tag's review. Its name is read from the
    /// node it writes. It belonged to the kit's
    /// footer, which drew it available through any write, and a press set
    /// the form's error. Enter, after each press, changes nothing either.
    #[gpui::test]
    async fn review_tag_waits_for_the_busy_look(cx: &mut TestAppContext) {
        use crate::workspace::BUSY_REASON;
        use crate::workspace::refusals::{self, Refusal};
        let fixture = tempfile::tempdir().unwrap();
        let (app, form, cx) = create_lightweight_tag(cx, fixture.path());
        // What the next frame built Review tag with: its announced and
        // drawn availability, whether its node is disabled and named by
        // why, and its tooltip.
        let frame = |cx: &mut VisualTestContext| {
            let (node, tooltip) = review_tag_frame(&form, cx);
            (
                refusals::get("review-tag"),
                node.role(),
                node.is_disabled(),
                node.label() == Some(review_tag_busy_name(BUSY_REASON).as_str()),
                tooltip,
            )
        };
        let state = |announced, dimmed, busy, tooltip| {
            (
                Some(Refusal { announced, dimmed }),
                gpui::accesskit::Role::Button,
                announced,
                busy,
                tooltip,
            )
        };
        let at_rest = state(false, false, false, None);
        let busy = true;
        let start = |cx: &mut VisualTestContext| {
            let held = app.update(cx, |app, _| app.operations.hold());
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.write(WriteCommand::StageAll, "Working…", window, cx)
                })
            });
            held
        };
        async fn release(
            app: &Entity<GitTurtle>,
            held: crate::operations::Held,
            cx: &mut VisualTestContext,
        ) {
            drop(held);
            let write = app
                .update(cx, |app, _| app.operation_task.take())
                .expect("the write");
            write.await;
            crate::views::tests::settle(app, cx).await;
        }
        let untouched = |cx: &mut VisualTestContext, at: &str| {
            assert!(
                cx.update(|window, cx| window.has_active_dialog(cx))
                    && cx.debug_bounds("create-tag-dialog-title").is_some(),
                "{at}: Create tag stays open"
            );
            let (error, pending) =
                form.read_with(cx, |form, cx| (form.error.clone(), form.pending(cx)));
            assert_eq!(
                (error, pending),
                (None, false),
                "{at}: the form is unchanged"
            );
            assert!(
                app.read_with(cx, |app, _| app.tag_actions.task.is_none()),
                "{at}: nothing is read"
            );
        };
        assert_eq!(frame(cx), at_rest, "at rest");

        // A quick write: announced unavailable while it runs, never dimmed.
        let held = start(cx);
        assert_eq!(frame(cx), state(true, false, busy, None), "a quick write");
        release(&app, held, cx).await;
        assert_eq!(frame(cx), at_rest, "after a quick write");

        // A slow write: a press is refused at once and changes nothing.
        let held = start(cx);
        assert_eq!(frame(cx), state(true, false, busy, None), "a write starts");
        let review = rendered(cx, "review-tag");
        cx.simulate_click(review.center(), Modifiers::default());
        frame(cx);
        untouched(cx, "pressed as the write starts");
        cx.simulate_keystrokes("enter");
        frame(cx);
        untouched(cx, "Enter as the write starts");
        cx.executor()
            .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
        cx.executor().run_until_parked();
        assert_eq!(
            frame(cx),
            state(true, true, busy, Some(BUSY_REASON)),
            "once the write looks busy"
        );
        cx.simulate_click(review.center(), Modifiers::default());
        frame(cx);
        untouched(cx, "pressed once the write looks busy");
        cx.simulate_keystrokes("enter");
        frame(cx);
        untouched(cx, "Enter once the write looks busy");
        release(&app, held, cx).await;
        assert_eq!(frame(cx), at_rest, "after the write");

        // Available again: Enter reads the tag's plan and opens its review.
        cx.simulate_keystrokes("enter");
        read_lands(&app, cx).await;
        draw(cx);
        assert!(
            cx.debug_bounds("operation-consequences").is_some(),
            "Review tag opens the tag's review once the write has ended"
        );
    }

    /// Create tag's Review tag says why it waits in the window's busy
    /// spell's own words (`GitTurtle::busy_reason`), in its name at once and
    /// its tooltip once it dims: while a request of the GitHub panel runs,
    /// that request's reason. It named "the current Git operation" whatever
    /// ran. No route opens Create tag while a request runs, nor the panel
    /// over Create tag, so the request is submitted here as the panel
    /// submits one; the reason has one source either way.
    #[gpui::test]
    async fn review_tag_gives_a_github_requests_reason(cx: &mut TestAppContext) {
        use crate::github_view::{RequestKind, submit_test_request};
        let fixture = tempfile::tempdir().unwrap();
        let (app, form, cx) = create_lightweight_tag(cx, fixture.path());
        for kind in RequestKind::ALL {
            // Held as a slow network holds it.
            let held = app.update(cx, |app, _| app.operations.hold());
            cx.update(|window, cx| {
                app.update(cx, |app, cx| submit_test_request(app, kind, window, cx))
            });
            let (node, tooltip) = review_tag_frame(&form, cx);
            let named = review_tag_busy_name(kind.reason());
            assert_eq!(
                (node.label(), tooltip),
                (Some(named.as_str()), None),
                "{kind:?}: its name says why at once"
            );
            cx.executor()
                .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
            cx.executor().run_until_parked();
            let (node, tooltip) = review_tag_frame(&form, cx);
            assert_eq!(
                (node.label(), tooltip),
                (Some(named.as_str()), Some(kind.reason())),
                "{kind:?}: and its tooltip once it dims"
            );
            drop(held);
            app.read_with(cx, |app, _| app.operations.drain());
            crate::views::tests::settle(&app, cx).await;
            assert_eq!(app.read_with(cx, |app, _| app.operation_busy), None);
        }
    }

    /// A plan read Settings cancels as it opens (`end_preparations`) ends
    /// the form's preparation with it: back on the repository, Review tag is
    /// announced and drawn available, and a press reads the plan again and
    /// opens the tag's review. The form kept a flag only the dropped reply
    /// cleared, so Review tag looked available and refused every press.
    #[gpui::test]
    async fn a_plan_read_dropped_off_the_page_leaves_review_tag_working(cx: &mut TestAppContext) {
        use crate::workspace::refusals::{self, Refusal};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(1480.), px(980.)));
        draw(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_create_tag(window, cx)));
        draw(cx);
        let form = app
            .read_with(cx, |app, _| app.tag_actions.draft.clone())
            .expect("Create tag opens");
        cx.update(|window, cx| {
            form.update(cx, |form, cx| {
                form.annotated = false;
                form.name
                    .update(cx, |name, cx| name.set_value("v9.0", window, cx))
            })
        });
        draw(cx);
        let review = rendered(cx, "review-tag");
        // The read waits behind a held executor until Settings shows.
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.simulate_click(review.center(), Modifiers::default());
        refusals::forget();
        draw(cx);
        assert!(
            form.read_with(cx, |form, cx| form.pending(cx)),
            "the plan is read"
        );
        assert_eq!(
            refusals::get("review-tag").map(|refusal| refusal.announced),
            Some(true),
            "Review tag is refused while its read runs"
        );
        // Settings opens over the dialog before the read lands, and
        // cancels it.
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
        assert!(
            app.read_with(cx, |app, _| app.tag_actions.task.is_none()),
            "Settings cancels the plan read"
        );
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        draw(cx);
        assert!(
            !form.read_with(cx, |form, cx| form.pending(cx)),
            "the dropped read leaves the form preparing"
        );
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_history(window, cx)));
        refusals::forget();
        draw(cx);
        assert!(
            cx.update(|window, cx| window.has_active_dialog(cx)),
            "Create tag stays open"
        );
        assert_eq!(
            refusals::get("review-tag"),
            Some(Refusal {
                announced: false,
                dimmed: false
            }),
            "Review tag is available on the repository again"
        );
        cx.simulate_click(review.center(), Modifiers::default());
        read_lands(&app, cx).await;
        draw(cx);
        assert!(
            cx.debug_bounds("operation-consequences").is_some(),
            "Review tag opens the tag's review"
        );
    }

    /// Cancel closes Create tag wherever focus is: with Settings shown over
    /// the open dialog, which takes focus, Cancel asked the focused element
    /// to cancel, which missed the dialog, so it stayed open.
    #[gpui::test]
    async fn cancel_closes_create_tag_with_settings_shown(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(1480.), px(980.)));
        draw(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_create_tag(window, cx)));
        draw(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
        draw(cx);
        let open =
            |cx: &mut VisualTestContext| cx.update(|window, cx| window.has_active_dialog(cx));
        assert!(open(cx), "Create tag stays open over Settings");
        let cancel = rendered(cx, "cancel-create-tag");
        cx.simulate_click(cancel.center(), Modifiers::default());
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(1));
        draw(cx);
        assert!(!open(cx), "Cancel closes Create tag");
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

    /// The summary's count agrees with its noun.
    #[test]
    fn the_summary_counts_one_tag_in_the_singular() {
        assert!(summary(1).starts_with("1 local tag · "), "{}", summary(1));
        assert!(summary(3).starts_with("3 local tags · "), "{}", summary(3));
    }

    /// Tags' text starts at one inset, where its filter's text and
    /// placeholder start, at 13 and 18 pt in a 1480 × 980 window and a
    /// 461 × 490 half tile: its title, its summary and each row's name. A
    /// row's name was centred in its row while the summary started at the
    /// dialog's edge. The placeholder fits its field in whole words: at 18
    /// pt in the half tile "Filter tags by name or object ID" was cut
    /// mid-glyph.
    #[gpui::test]
    async fn tags_text_starts_at_the_filters_inset(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let (_repo, app, cx) = small_window_with_tags(cx, fixture.path(), 0);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_tags(window, cx)));
        read_lands(&app, cx).await;
        draw(cx);
        let query = app
            .read_with(cx, |app, _| app.tag_actions.filter.clone())
            .expect("Tags' filter");
        for (width, height) in [(1480., 980.), (461., 490.)] {
            for size in [13, 18] {
                cx.simulate_resize(size_of(width, height));
                set_interface_text_size(cx, &app, size);
                let at = format!("{size} pt in {width} × {height}");
                let filter = cx.read(|cx| query.read(cx).input_bounds().left());
                for selector in ["tags-dialog-title", "tag-list-summary", "tag-row-text-0"] {
                    let start = rendered(cx, selector).left();
                    assert!(
                        (start - filter).abs() < px(0.01),
                        "{at}: {selector} starts at {start:?}, the filter's text at {filter:?}"
                    );
                }
                let row = rendered(cx, "tag-row-text-0");
                assert!(
                    row.size.width > px(100.),
                    "{at}: the row's name {row:?} fills its row from the inset"
                );
                let (shown, room) = cx.read(|cx| {
                    let query = query.read(cx);
                    (
                        query.presentation().placeholder().clone(),
                        query.text_bounds().map(|bounds| bounds.size.width),
                    )
                });
                let expected = if width < 1000. && size == 18 {
                    "Filter tags…"
                } else {
                    FILTER_PLACEHOLDERS[0]
                };
                assert_eq!(shown.as_ref(), expected, "{at}: in a field {room:?} wide");
            }
        }
        set_interface_text_size(cx, &app, appearance::DEFAULT_INTERFACE_TEXT_SIZE);
    }

    fn size_of(width: f32, height: f32) -> Size<Pixels> {
        size(px(width), px(height))
    }

    /// Tab moves through every row of Tags and Shift+Tab back, and the list
    /// scrolls each row into view with its focus ring. Once a row is shown,
    /// redraws and a click on a partly visible row scroll nothing.
    #[gpui::test]
    async fn tab_reveals_every_tag_row(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let (_repo, app, cx) = small_window_with_tags(cx, fixture.path(), 27);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_tags(window, cx)));
        read_lands(&app, cx).await;
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
        read_lands(&app, cx).await;
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
        read_lands(&app, cx).await;
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
        read_lands(&app, cx).await;
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
        read_lands(&app, cx).await;
        draw(cx);
        let list = app
            .read_with(cx, |app, _| app.tag_actions.list.clone())
            .expect("the Push to… list");
        (app, list, cx)
    }

    /// A list `height` tall that keeps `room` around its `rows` rows, each
    /// 20 px tall, cued by [`ScrollCue`] as the app's lists are. Tests find
    /// it as `cued-list`, its rows as `cued-row-<n>` and its scrollbar, while
    /// it is drawn, as `cued-list/scrollbar`.
    struct Cued {
        scroll: ScrollHandle,
        height: Pixels,
        room: Pixels,
        rows: usize,
    }

    impl Render for Cued {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let cue = ScrollCue::new(&self.scroll, self.room);
            let observe = cue.observe(|_, _, _| {});
            let list = div()
                .id("cued-list")
                .debug_selector(|| "cued-list".into())
                .h(self.height)
                .p(self.room)
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .children((0..self.rows).map(|row| {
                    div()
                        .debug_selector(move || format!("cued-row-{row}"))
                        .h(px(20.))
                        .child(format!("Row {row}"))
                }));
            div().w(px(200.)).child(
                cue.list(list, "cued-list/scrollbar")
                    .on_children_prepainted(observe),
            )
        }
    }

    /// A window drawing a [`Cued`] list `height` tall with `rows` rows and
    /// `room` around them, in the Midnight palette.
    fn cued_window(
        cx: &mut gpui_kit::TestAppContext,
        height: f32,
        room: Pixels,
        rows: usize,
    ) -> (Entity<Cued>, &mut VisualTestContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| appearance::ThemeChoice::Midnight.apply(None, cx));
        let (list, cx) = cx.add_window_view(|_, _| Cued {
            scroll: ScrollHandle::new(),
            height: px(height),
            room,
            rows,
        });
        draw(cx);
        (list, cx)
    }

    /// The fades the first frame drawn after `input` paints over the list
    /// drawn at `list` ([`painted_fades`]). `input` runs and the frame is
    /// drawn in one update, and the frame's fades are read before the test
    /// harness, flushing the effects the input left, draws the window again,
    /// which would hide a fade that lagged a frame behind.
    fn first_frame_fades(
        cx: &mut VisualTestContext,
        list: Bounds<Pixels>,
        input: impl FnOnce(&mut Window, &mut App),
    ) -> (Option<Pixels>, Option<Pixels>) {
        cx.update(|window, cx| {
            input(window, cx);
            window.draw(cx).clear(cx);
            fades_in(window, cx, list)
        })
    }

    /// The edge fades painted over the list drawn at `list`: the height of
    /// the one along its top and of the one along its bottom, each as wide
    /// as the list and in the palette's canvas, the theme's background,
    /// opaque at the list's edge and clear inward.
    pub(crate) fn painted_fades(
        cx: &mut VisualTestContext,
        list: Bounds<Pixels>,
    ) -> (Option<Pixels>, Option<Pixels>) {
        cx.update(|window, cx| fades_in(window, cx, list))
    }

    /// [`painted_fades`] in the frame `window` drew last.
    fn fades_in(
        window: &Window,
        cx: &App,
        list: Bounds<Pixels>,
    ) -> (Option<Pixels>, Option<Pixels>) {
        let surface: Hsla = rgb(appearance::palette(cx).canvas).into();
        // A gradient toward `angle` (0° the top, 180° the bottom), clear
        // where it starts and the surface where it ends.
        let toward = |angle: f32| {
            linear_gradient(
                angle,
                linear_color_stop(surface.opacity(0.), 0.),
                linear_color_stop(surface, 1.),
            )
        };
        let (up, down) = (toward(0.), toward(180.));
        let near = |a: Pixels, b: Pixels| (a - b).abs() < px(0.5);
        let scale = window.scale_factor();
        let (mut top, mut bottom) = (None, None);
        for quad in window.painted_quads() {
            let band = logical(quad.bounds, scale);
            if !near(band.left(), list.left()) || !near(band.right(), list.right()) {
                continue;
            }
            if quad.background == up && near(band.top(), list.top()) {
                assert!(top.is_none(), "one fade along {list:?}'s top");
                top = Some(band.size.height);
            } else if quad.background == down && near(band.bottom(), list.bottom()) {
                assert!(bottom.is_none(), "one fade along {list:?}'s bottom");
                bottom = Some(band.size.height);
            }
        }
        (top, bottom)
    }

    /// A list that keeps room around its rows is cued where a thumb moves,
    /// which it judges by the track its scrollbar lies along, the list less
    /// that room, which is where the kit lays it: 60 px tall with 8 px
    /// around its rows, a 44 px track shorter than [`SHORTEST_SCROLL_TRACK`],
    /// it draws no scrollbar, and 80 px tall, a 64 px track, it draws one.
    /// Judged by the list's whole height, the 60 px list drew a thumb along a
    /// track shorter than the thumb. Every [`ScrollCue`] follows the rule; it
    /// was once a list's own choice. At either length its overflowing rows
    /// keep the scrollbar's gutter, so they never move as it crosses.
    #[gpui::test]
    fn a_cue_judges_the_track_its_scrollbar_lies_along(cx: &mut gpui_kit::TestAppContext) {
        const ROOM: Pixels = px(8.);
        let (list, cx) = cued_window(cx, 60., ROOM, 20);
        let bar = Scrollbar::width();
        for (height, cued) in [(60., false), (80., true), (60., false)] {
            list.update(cx, |list, cx| {
                list.height = px(height);
                cx.notify();
            });
            draw(cx);
            let bounds = cx.debug_bounds("cued-list").expect("the list");
            let row = cx.debug_bounds("cued-row-0").expect("its first row");
            assert!(
                row.right() < bounds.right() - ROOM - px(0.5),
                "{height} px: the overflowing row {row:?} keeps a gutter in {bounds:?}"
            );
            // The scrollbar's viewport, which its track fills.
            let track = Bounds::new(
                point(bounds.right() - ROOM - bar - px(4.), bounds.top() + ROOM),
                size(bar + px(4.), bounds.size.height - ROOM * 2.),
            );
            let thumb = painted_thumb(cx, track);
            assert_eq!(
                thumb.is_some(),
                cued,
                "{height} px: a scrollbar ({thumb:?}) only along a track a thumb moves on"
            );
            assert_eq!(
                cx.debug_bounds("cued-list/scrollbar").is_some(),
                cued,
                "{height} px: the scrollbar's element is drawn only along a track a thumb moves on"
            );
            assert_eq!(
                painted_fades(cx, bounds).1.is_some(),
                !cued,
                "{height} px: a list at its top fades its bottom edge only where it draws no scrollbar"
            );
        }
    }

    /// A list too short for the kit's thumb to move along its track draws no
    /// scrollbar and cues the rows beyond its edges with a fade at each edge
    /// they continue past: at 13 and 18 pt, 40 px tall with ten 20 px rows,
    /// it leaves no scrollbar element behind, its rows keep the gutter they
    /// keep beside a scrollbar, so they never move as the list grows or
    /// shrinks past the shortest track, and at its top it fades its bottom
    /// edge alone, part way down both,
    /// and scrolled to its end its top alone. Each fade is the scaled 8 px,
    /// at most a quarter of the list, so it grows with the text, and lies
    /// across the list's whole width. A side header's controls region at
    /// its 36–44 px floor once drew the kit's 48 px thumb along a track
    /// shorter than the thumb.
    #[gpui::test]
    fn a_list_too_short_for_a_thumb_fades_the_edges_its_rows_continue_past(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        let (list, cx) = cued_window(cx, 40., Pixels::ZERO, 10);
        let scroll = list.read_with(cx, |list, _| list.scroll.clone());
        let mut fades = Vec::new();
        for text in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            scroll_list_to(cx, &scroll, 0.);
            let at = format!("{text} pt");
            let region = cx.debug_bounds("cued-list").expect("the list");
            assert!(
                (region.size.height - px(40.)).abs() < px(0.01) && scroll.max_offset().y > px(100.),
                "{at}: the 40 px list {region:?} overflows"
            );
            let bar = Scrollbar::width() + px(4.);
            let track = Bounds::new(
                point(region.right() - bar, region.top()),
                size(bar, region.size.height),
            );
            assert!(
                cx.debug_bounds("cued-list/scrollbar").is_none()
                    && painted_thumb(cx, track).is_none(),
                "{at}: the list {region:?} draws no scrollbar"
            );
            let row = cx.debug_bounds("cued-row-0").expect("its first row");
            let ring = appearance::BUTTON_FOCUS_RING;
            let gutter = Scrollbar::width() + ring.gap + ring.width;
            assert!(
                (row.right() - (region.right() - gutter)).abs() < px(0.5),
                "{at}: the row {row:?} keeps the {gutter:?} gutter it keeps beside a scrollbar in {region:?}"
            );
            let fade = cx
                .update(|_, _| appearance::ui_size(8.))
                .min(region.size.height / 4.);
            let near = |painted: Option<Pixels>| {
                painted.is_some_and(|height| (height - fade).abs() < px(0.01))
            };
            let (top, bottom) = painted_fades(cx, region);
            assert!(
                top.is_none() && near(bottom),
                "{at}: at its top the list fades its bottom edge alone by {fade:?}, not {top:?} and {bottom:?}"
            );
            scroll_list_to(cx, &scroll, 0.5);
            let (top, bottom) = painted_fades(cx, region);
            assert!(
                near(top) && near(bottom),
                "{at}: part way down the list fades both edges, not {top:?} and {bottom:?}"
            );
            scroll_list_to(cx, &scroll, 1.);
            let (top, bottom) = painted_fades(cx, region);
            assert!(
                near(top) && bottom.is_none(),
                "{at}: scrolled to its end the list fades its top edge alone, not {top:?} and {bottom:?}"
            );
            fades.push(fade);
        }
        assert!(
            fades[1] > fades[0],
            "the fade grows with the text: {fades:?}"
        );
    }

    /// A short list's fades follow what it shows in the frame that shows it,
    /// and catch no pointer. Rows that arrive in a list that fit fade its
    /// bottom edge in the first frame that draws them: decided as the cue
    /// rendered, from the reach the list kept from the frame before, they
    /// drew none, and nothing drew the list again. A wheel over the bottom
    /// fade scrolls the rows beneath it, and the frame it draws fades both
    /// edges; one to the end leaves the top's alone, and one back to the top
    /// the bottom's alone, each in that one frame.
    #[gpui::test]
    fn a_short_list_s_fades_follow_its_rows_and_a_wheel_in_the_frame_drawn(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        let (list, cx) = cued_window(cx, 40., Pixels::ZERO, 1);
        let region = cx.debug_bounds("cued-list").expect("the list");
        assert_eq!(
            painted_fades(cx, region),
            (None, None),
            "a list whose rows fit fades no edge"
        );
        let grown = list.clone();
        let (top, bottom) = first_frame_fades(cx, region, move |_, cx| {
            grown.update(cx, |list, cx| {
                list.rows = 10;
                cx.notify();
            })
        });
        assert!(
            top.is_none() && bottom.is_some(),
            "the frame that draws the rows that arrived fades the bottom edge alone, not {top:?} and {bottom:?}"
        );
        let wheel = |by: f32, cx: &mut VisualTestContext| {
            first_frame_fades(cx, region, move |window, cx| {
                window.dispatch_event(
                    PlatformInput::ScrollWheel(ScrollWheelEvent {
                        // Over the bottom fade.
                        position: point(region.center().x, region.bottom() - px(2.)),
                        delta: ScrollDelta::Pixels(point(px(0.), px(by))),
                        modifiers: Modifiers::default(),
                        touch_phase: TouchPhase::Moved,
                    }),
                    cx,
                );
            })
        };
        let first = cx.debug_bounds("cued-row-0").expect("the first row");
        let (top, bottom) = wheel(-15., cx);
        let moved = cx.debug_bounds("cued-row-0").expect("the first row");
        assert!(
            (first.top() - moved.top() - px(15.)).abs() < px(0.5),
            "a wheel over the bottom fade scrolls the rows beneath it: {first:?} to {moved:?}"
        );
        assert!(
            top.is_some() && bottom.is_some(),
            "the frame the wheel draws fades both edges, not {top:?} and {bottom:?}"
        );
        let (top, bottom) = wheel(-10_000., cx);
        let last = cx.debug_bounds("cued-row-9").expect("the last row");
        assert!(
            (last.bottom() - region.bottom()).abs() < px(0.5),
            "the wheel scrolls to the rows' end {last:?} in {region:?}"
        );
        assert!(
            top.is_some() && bottom.is_none(),
            "the frame the wheel to the end draws fades the top edge alone, not {top:?} and {bottom:?}"
        );
        let (top, bottom) = wheel(10_000., cx);
        assert!(
            top.is_none() && bottom.is_some(),
            "the frame the wheel back draws fades the bottom edge alone, not {top:?} and {bottom:?}"
        );
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
