//! Authoritative native conversations and independent, captured reply drafts.
use super::*;
use crate::github::{
    conversations::{CapturedThread, CursorPage, Thread},
    drafts::ReplyDraft,
};

/// Copy reply's mark, keyed by the conversation the reply answers.
fn reply_mark(target: &CapturedThread) -> copy_feedback::Mark {
    copy_feedback::Mark::new(
        "github-copy-reply",
        (
            &target.account_id,
            &target.repository_id,
            &target.pull_id,
            &target.thread_id,
        ),
    )
}

pub(super) struct State {
    pub threads: Vec<Thread>,
    pub cursors: Vec<Option<String>>,
    pub next_cursor: Option<String>,
    pub total: u32,
    pub loaded: bool,
    pub pagination_notice: Option<String>,
    pub invalid_threads: std::collections::HashSet<String>,
    pub active: Option<CapturedThread>,
    pub input: Entity<TextareaState>,
    pub deferred: DeferredDraft,
    pub timer: Option<Task<()>>,
    /// The outbound confirmation's stop, the region that scrolls its destination and text,
    /// which takes focus as the confirmation opens ([`Panel::render_confirmation`]).
    pub confirm_focus: FocusHandle,
    /// The scroll of the confirmation's region, at its top as each confirmation opens.
    pub confirm_scroll: ScrollHandle,
    /// What the last frame found of the confirmation's lines and the body's view ([`ConfirmFit`]).
    pub confirm_fit: std::rc::Rc<std::cell::Cell<ConfirmFit>>,
    /// The confirmation's form, decided before layout for the width, font and text it was
    /// decided for ([`ConfirmForm`]).
    pub confirm_form: std::cell::RefCell<Option<ConfirmForm>>,
    /// The conversation list's scroll, which Tab moves to show a conversation's actions
    /// (`Panel::conversation_part`), and the focus it last showed.
    pub list_scroll: ScrollHandle,
    pub list_revealed: std::rc::Rc<std::cell::RefCell<super::review::FocusObservation>>,
    /// Where the list's blocks start: its conversations and their rows, each comment with
    /// its author's line, so a reveal never cuts one at the list's top.
    pub list_blocks: super::review::Blocks,
    /// What the conversations pane draws besides its list, laid out last frame: the list
    /// keeps the pane inside the panel's body.
    pub list_chrome: std::rc::Rc<std::cell::Cell<Pixels>>,
    /// The panel's notice where it concerns one conversation, with that conversation: a reply
    /// sent or restored, a conversation resolved or reopened. Reply on that conversation keeps
    /// the notice; Reply on any other clears it ([`Panel::begin_reply`]).
    pub notice: Option<(String, CapturedThread)>,
    /// The outcome the open composer showed last frame, if any, which the body has scrolled
    /// into view ([`Panel::reveal_outcome`]).
    pub outcome_shown: std::rc::Rc<std::cell::RefCell<Option<Shown>>>,
}
/// What the last frame found of the outbound confirmation besides its form
/// ([`Panel::render_confirmation`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct ConfirmFit {
    /// How far apart its 12 pt lines are drawn ([`confirm_line`]), which the frame that drew
    /// it read from the text style, for the keys that scroll the region a line.
    pub line: Pixels,
    /// How tall the top line stood above the body in the last frame, none while it stood among
    /// the content: a frame that changes it moves the region, which the top line's own shift
    /// of the body's scroll reveals nothing for, so the frame reveals a focused region again,
    /// where the last frame showed any of it ([`Self::in_view`]).
    pub top_line: Pixels,
    /// How tall the top line stood among the content in the last frame, none while it stood
    /// above the body: it lies above the confirmation there, so a frame that changes it moves
    /// the region inside a body of the same size, and that frame reveals a focused region again
    /// too, where the last frame showed any of it ([`Self::in_view`]).
    pub line_among: Pixels,
    /// Whether the last frame drew the region or Send and Keep editing at least partly inside
    /// the body's view. Where it drew neither, the person has scrolled the body away from them,
    /// and a change of the top line's height, above the body or among the content, leaves the
    /// body where it is.
    pub in_view: bool,
}

/// The sentence the confirmation stands above Send, or in its region after the destination.
const GUIDANCE: &str = "Send performs one outbound action with these exact captured targets and comments. Inspect the destination before repeating an uncertain attempt.";

/// What the outbound confirmation draws besides its region, decided before layout in the frame
/// being drawn from the panel's width and the interface font, so the frame that lays it out
/// decides where the guidance stands and how tall the region may be from what it draws
/// ([`Panel::render_confirmation`]). Decided from the last frame's layout, a new panel's first
/// long confirmation at 18 pt in a 461 × 490 window drew its guidance below the region and the
/// region as tall as if no buttons stood below it, and a long reply's in a window made only
/// narrower, 1,480 × 973 to 461 × 973 at 13 pt, kept the room of the guidance's fewer,
/// wider lines and laid the region out 612 px tall where it settles at 573, each for one
/// frame. It is decided again only when what it was decided for changes
/// ([`State::confirm_form`]).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ConfirmForm {
    key: FormKey,
    /// The guidance sentence's height, its 12 pt lines wrapped at the width, wherever it is
    /// drawn.
    pub guidance: Pixels,
    /// Everything the confirmation's content draws besides its region and the guidance: the
    /// room around the region for its ring, the row or two rows of Send and Keep editing, and
    /// the gaps between them.
    pub chrome: Pixels,
    /// How tall the text of the panel's top line stands while a conversation is open, its 12 pt
    /// lines wrapped at the width they are laid out in ([`top_line_width`]) and the gaps between
    /// them, before the cap a tall line scrolls under ([`pinned_line_height`]); none where it
    /// says nothing.
    pub top_text: Pixels,
    /// How tall the label of the top line's busy row stands while a conversation is open, its
    /// 12 pt lines wrapped at the width the row leaves it beside Cancel; none where no busy row
    /// stands there ([`pinned_line_height`]).
    pub busy_text: Pixels,
}
#[cfg(test)]
impl ConfirmForm {
    /// The width it was decided at ([`confirm_width`]).
    pub(super) fn width(&self) -> Pixels {
        self.key.width
    }
}
/// What a [`ConfirmForm`] was decided for.
#[derive(Clone, Debug, PartialEq)]
struct FormKey {
    /// The width the confirmation lays its content out in ([`confirm_width`]).
    width: Pixels,
    /// The interface font, its size and its line ([`confirm_line`]), the rem size the gaps
    /// follow, and the display's scale.
    font: Font,
    size: Pixels,
    line: Pixels,
    rem: Pixels,
    scale: f32,
    /// Send's label.
    send: &'static str,
    /// What the panel's top line says while a conversation is open ([`TopLineTexts`]).
    top_line: Vec<String>,
    /// What its busy row says, where one stands there.
    busy: Option<&'static str>,
}

/// What the panel's top line says while a conversation is open, in the order
/// [`Panel::pin_top_line`] draws it: its notice, the failure or cancel that answers there, and
/// refused input. All none while no conversation is open, where the line stands among the
/// content and takes none of the room above the body ([`Panel::pinned_line_texts`]).
pub(super) type TopLineTexts<'a> = [Option<&'a str>; 3];

thread_local! {
    /// Send's label at a compact Button's 12 pt, laid out once per font and size
    /// ([`crate::label_text_width`]).
    static SEND_LABEL: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    /// Keep editing's, likewise.
    static KEEP_LABEL: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    /// The busy row's Cancel's, likewise.
    static CANCEL_LABEL: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
thread_local! {
    /// How many times a confirmation's form was decided, its guidance shaped, for a test.
    pub(super) static CONFIRM_FORMS_DECIDED: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

impl State {
    /// The confirmation's form in `window`, whose text style is `style`, with Send reading
    /// `send`, the top line saying `top_line` and its busy row, where one stands there, saying
    /// `busy`: the guidance's height, the rest of the confirmation's, the top line's text's and
    /// its busy row's label's ([`ConfirmForm`]), the ones last decided where they were decided
    /// for the same width, font, sizes, scale, label, top line and busy row, so a frame that
    /// changes none of them shapes nothing and allocates nothing, and otherwise decided now:
    /// the guidance, the top line's text and the busy row's label shaped with the text system's
    /// wrap at the widths they are laid out in, Send and Keep editing beside each other where
    /// their measured labels fit the width ([`crate::placed_button_width`]) and on two rows
    /// where not, as the actions' row wraps them, and the room around the region, the Buttons'
    /// frames and the gaps from their sizes, each on the device pixel layout places it.
    pub(super) fn confirm_form(
        &self,
        style: &TextStyle,
        send: &'static str,
        top_line: TopLineTexts<'_>,
        busy: Option<&'static str>,
        window: &Window,
        cx: &App,
    ) -> (Pixels, Pixels, Pixels, Pixels) {
        let width = confirm_width(window, cx);
        let size = appearance::ui_text(12.);
        let line = confirm_line(style, window);
        let (rem, scale) = (window.rem_size(), window.scale_factor());
        let mut kept = self.confirm_form.borrow_mut();
        if let Some(form) = kept.as_ref().filter(|form| {
            let key = &form.key;
            key.width == width
                && key.size == size
                && key.line == line
                && key.rem == rem
                && key.scale == scale
                && key.send == send
                && key.busy == busy
                && key.font.family == style.font_family
                && key.font.features == style.font_features
                && key.font.fallbacks == style.font_fallbacks
                && key.font.weight == style.font_weight
                && key.font.style == style.font_style
                && key
                    .top_line
                    .iter()
                    .map(String::as_str)
                    .eq(top_line.iter().flatten().copied())
        }) {
            return (form.guidance, form.chrome, form.top_text, form.busy_text);
        }
        #[cfg(test)]
        CONFIRM_FORMS_DECIDED.with(|decided| decided.set(decided.get() + 1));
        let snap = |length: Pixels| window.pixel_snap(length);
        let gap = snap(rem * 0.5);
        // A shaping failure counts as more lines than any body holds beside the region.
        let shaped = |text: &str, width: Pixels| {
            let lines = crate::image_compare::wrapped_lines(text, size, width, window);
            line * lines.min(usize::from(u16::MAX)) as f32
        };
        // Shaped with GPUI's wrap at the width its label is laid out at, the region's or the
        // confirmation's, which are one width: the region keeps no gutter.
        let guidance = shaped(GUIDANCE, width);
        let wrap = top_line_width(window, cx);
        let top_text =
            top_line
                .iter()
                .flatten()
                .enumerate()
                .fold(Pixels::ZERO, |height, (index, text)| {
                    height + shaped(text, wrap) + if index > 0 { gap } else { Pixels::ZERO }
                });
        let placed = |label: &str, cache| {
            crate::placed_button_width(crate::label_text_width(label, cache, window), false, window)
        };
        // The row's label has what Cancel and the gap beside it leave of the line's width.
        let busy_text = busy.map_or(Pixels::ZERO, |busy| {
            shaped(busy, wrap - gap - placed("Cancel", &CANCEL_LABEL))
        });
        let rows = if placed(send, &SEND_LABEL) + gap + placed("Keep editing", &KEEP_LABEL) <= width
        {
            1.
        } else {
            2.
        };
        let chrome = snap(appearance::button_ring_room(cx)) * 2.
            + gap
            + snap(appearance::ui_size(28.)) * rows
            + gap * (rows - 1.);
        let form = kept.insert(ConfirmForm {
            key: FormKey {
                width,
                font: style.font(),
                size,
                line,
                rem,
                scale,
                send,
                top_line: top_line
                    .iter()
                    .flatten()
                    .map(|text| (*text).to_owned())
                    .collect(),
                busy,
            },
            guidance,
            chrome,
            top_text,
            busy_text,
        });
        (form.guidance, form.chrome, form.top_text, form.busy_text)
    }
}

/// The width the panel's top line wraps its text at while a conversation is open, in `window`:
/// the panel's width less the dialog's 1 px edge and its side padding at either side, the
/// scroll gutter the line keeps at its right ([`crate::tags::ScrollCue::gutter`]) and the
/// panel's text inset at its left, as [`Panel::pin_top_line`] lays it out.
fn top_line_width(window: &Window, cx: &App) -> Pixels {
    let snap = |length: Pixels| window.pixel_snap(length);
    snap(super::panel_width(window))
        - (snap(px(1.)) + snap(gpui_kit::component::dialog::dialog_side_padding(window, cx))) * 2.
        - snap(crate::tags::ScrollCue::gutter())
        - snap(super::text_inset())
}

/// How tall the panel's top line stands above the body in `window` while a conversation is
/// open, decided before layout as the guidance is: `text`, the height of what it says
/// ([`ConfirmForm::top_text`]), under the cap past which it scrolls, half the body's room, then
/// the busy row where `busy_text`, its label's wrapped lines ([`ConfirmForm::busy_text`]), says
/// one stands there, as tall as the taller of those lines and Cancel, a compact Button's
/// height, and the gap between them, the padding under them and the divider, each on the device
/// pixel layout places it, as [`Panel::pin_top_line`] lays them out; none where the line holds
/// nothing.
pub(super) fn pinned_line_height(text: Pixels, busy_text: Pixels, window: &Window) -> Pixels {
    let said = text > Pixels::ZERO;
    let busy = busy_text > Pixels::ZERO;
    if !said && !busy {
        return Pixels::ZERO;
    }
    let snap = |length: Pixels| window.pixel_snap(length);
    let gap = snap(window.rem_size() * 0.5);
    let text = text.min(snap(super::body_max_height(window) / 2.));
    let tall = busy_text.max(snap(appearance::ui_size(28.)));
    let row = match (busy, said) {
        (true, true) => gap + tall,
        (true, false) => tall,
        (false, _) => Pixels::ZERO,
    };
    text + row + gap + snap(px(1.))
}

/// The width the confirmation lays out its region, its guidance and its Buttons in, in
/// `window`, from the panel's width ([`super::panel_width`]): less the dialog's 1 px edge and
/// its side padding at either side, the gutter the body keeps for its scrollbar past the room
/// its rings take there, and the confirmation's own padding and border at either side, each a
/// length layout places on a device pixel. Read from the window rather than the last frame's
/// layout, so the frame a resize draws decides the form from the width it lays out.
fn confirm_width(window: &Window, cx: &App) -> Pixels {
    let snap = |length: Pixels| window.pixel_snap(length);
    let (edge, ring) = (snap(px(1.)), appearance::button_ring_room(cx));
    snap(super::panel_width(window))
        - (edge + snap(gpui_kit::component::dialog::dialog_side_padding(window, cx))) * 2.
        - (snap(ring + crate::tags::ScrollCue::gutter()) - snap(ring))
        - (snap(super::text_inset() - px(1.)) + edge) * 2.
}

/// How far apart the confirmation's 12 pt lines are drawn: the line height of `style`, the
/// window's text style, at that size, snapped to a device pixel, as a text element lays its
/// lines out. Read from the style as each frame renders, so a confirmation's first frame keeps
/// the region's two-line floor: read from the last frame's measure, a new panel's first
/// confirmation had none.
fn confirm_line(style: &TextStyle, window: &Window) -> Pixels {
    window.pixel_snap(
        style
            .line_height
            .to_pixels(appearance::ui_text(12.).into(), window.rem_size()),
    )
}

/// The confirmation region's cue that its destination and text continue past an edge: the
/// shared scroll cue's edge fades ([`crate::tags::ScrollCue`]), a band
/// [`crate::tags::EDGE_FADE`] tall at the interface size and at most a quarter of the region, at
/// each edge more lies past, opaque at the edge in the dialog's surface, the palette's canvas,
/// and clear inward; none at an end the region is scrolled to, none where what it holds fits,
/// and none in a region shorter than one fade. Unlike a list's cue it draws no scrollbar at any
/// length and keeps no gutter, so what the region holds keeps its width whether or not it
/// overflows, and a confirmation's first frame lays out as it settles. It reads the region's
/// bounds, offset and reach as it paints, after the region has laid them out in that frame, so
/// a key, a wheel or a resize moves the fades in the frame that scrolls it, and it inserts no
/// hitbox, so a wheel over a fade scrolls the region. It lies inside the region, and the
/// region's ring on the room around it, so no fade covers the ring. Cut at its foot with no
/// cue, at 18 pt in a 461 × 490 window nothing said the guidance and the text Send sends lay
/// below.
fn region_fades(scroll: ScrollHandle) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _, window, cx| {
            let region = scroll.bounds();
            let full = appearance::ui_size(crate::tags::EDGE_FADE);
            if region.size.height < full {
                return;
            }
            let fade = full.min(region.size.height / 4.);
            // Half a pixel either way is an end, as a scrolling list's.
            let slack = px(0.5);
            let scrolled = -scroll.offset().y;
            let surface: Hsla = rgb(palette(cx).canvas).into();
            let band = |angle: f32| {
                linear_gradient(
                    angle,
                    linear_color_stop(surface.opacity(0.), 0.),
                    linear_color_stop(surface, 1.),
                )
            };
            if scrolled > slack {
                // Toward the top, opaque at the region's top edge.
                let top = Bounds::new(region.origin, size(region.size.width, fade));
                window.paint_quad(fill(top, band(0.)));
            }
            if scrolled < scroll.max_offset().y - slack {
                // Toward the bottom, opaque at the region's bottom edge.
                let bottom = Bounds::new(
                    point(region.left(), region.bottom() - fade),
                    size(region.size.width, fade),
                );
                window.paint_quad(fill(bottom, band(180.)));
            }
        },
    )
    .absolute()
    .size_full()
}

/// One frame of the outbound confirmation as it prepainted, for tests: the room around the
/// region its ring lies on, where Send and Keep editing lie, the body's view, whether the
/// guidance stood in the region, and what the confirmation drew besides the region and the
/// guidance ([`ConfirmForm::chrome`]).
#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub(super) struct ConfirmFrame {
    pub ring: Bounds<Pixels>,
    pub actions: Bounds<Pixels>,
    pub body: Bounds<Pixels>,
    pub inside: bool,
    pub chrome: Pixels,
}
#[cfg(test)]
thread_local! {
    /// Every frame of the confirmation drawn since a test last took them.
    pub(super) static CONFIRM_FRAMES: std::cell::RefCell<Vec<ConfirmFrame>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// What the outbound confirmation confirms, which names its region for assistive technologies:
/// the confirmation draws no title, so the name says what it is for rather than repeat one.
fn confirmation_title(action: &Action) -> &'static str {
    match action {
        Action::Create(_) => "Confirm the new pull request",
        Action::Comment { .. } => "Confirm the comment",
        Action::Review(_) => "Confirm the review",
        Action::Reply { .. } => "Confirm the reply",
        Action::SetThreadResolved { resolved: true, .. } => "Confirm resolving the conversation",
        Action::SetThreadResolved {
            resolved: false, ..
        } => "Confirm reopening the conversation",
    }
}

/// An outcome the composer shows: its failure, or that Cancel stopped its request, and how many
/// presses the failure line has answered, so the same refusal pressed again is shown again.
pub(super) type Shown = (Option<String>, Option<&'static str>, usize);
impl State {
    pub fn new(window: &mut Window, cx: &mut Context<Panel>) -> Self {
        Self {
            threads: vec![],
            cursors: vec![None],
            next_cursor: None,
            total: 0,
            loaded: false,
            pagination_notice: None,
            invalid_threads: Default::default(),
            active: None,
            input: cx.new(|cx| {
                TextareaState::new(window, cx)
                    // Bounded prose layout lets Tab traverse controls instead of indenting.
                    .auto_grow(3, 3)
                    .placeholder("Write a reply to this conversation…")
            }),
            deferred: DeferredDraft::default(),
            timer: None,
            // A Tab stop: GPUI ignores the `tab_stop` of the div that tracks this handle, so
            // the handle carries it.
            confirm_focus: cx.focus_handle().tab_stop(true),
            confirm_scroll: ScrollHandle::new(),
            confirm_fit: Default::default(),
            confirm_form: Default::default(),
            list_scroll: ScrollHandle::new(),
            list_revealed: Default::default(),
            list_blocks: Default::default(),
            list_chrome: Default::default(),
            notice: None,
            outcome_shown: Default::default(),
        }
    }
    pub fn reset_content(&mut self) {
        self.threads.clear();
        self.cursors = vec![None];
        self.next_cursor = None;
        self.total = 0;
        self.loaded = false;
        self.pagination_notice = None;
        self.invalid_threads.clear();
        self.active = None;
        self.notice = None;
        self.list_scroll.set_offset(point(px(0.), px(0.)));
    }
}

/// A thread's line counting the comments it shows of all it has, the
/// total agreeing with its noun ("1 of 1 comment", where it read "1 of 1
/// comments").
fn comments_shown(shown: usize, total: u32) -> String {
    format!(
        "{shown} of {}",
        crate::counted(total, "comment", "comments")
    )
}

fn reply_matches(thread: &Thread, target: &CapturedThread, account: Option<&str>) -> bool {
    thread.target == *target && account == Some(target.account.as_str()) && thread.reply_available()
}

fn accept_page_cursor(
    history: &mut Vec<Option<String>>,
    cursor: Option<String>,
    next: Option<String>,
) -> Option<String> {
    if let Some(index) = history.iter().position(|old| old == &cursor) {
        history.truncate(index + 1);
    } else {
        history.push(cursor);
    }
    next.filter(|next| {
        history.len() < 1000 && !history.iter().any(|old| old.as_ref() == Some(next))
    })
}

impl Panel {
    pub(super) fn conversation_bytes(&self, cx: &App) -> usize {
        self.conversations
            .threads
            .iter()
            .map(Thread::retained_bytes)
            .sum::<usize>()
            + self
                .conversations
                .cursors
                .iter()
                .filter_map(Option::as_ref)
                .map(String::len)
                .sum::<usize>()
            + self
                .conversations
                .next_cursor
                .as_ref()
                .map_or(0, String::len)
            + self
                .conversations
                .active
                .as_ref()
                .map_or(0, CapturedThread::retained_bytes)
            + self.conversations.input.read(cx).value().len() * 3
            + self
                .conversations
                .invalid_threads
                .iter()
                .map(String::len)
                .sum::<usize>()
            + self
                .conversations
                .pagination_notice
                .as_ref()
                .map_or(0, String::len)
            + self
                .conversations
                .deferred
                .draft
                .as_ref()
                .map_or(0, |d| match d {
                    Draft::Reply(r) => r.body.len() + r.target.retained_bytes(),
                    _ => 0,
                })
    }
    pub(super) fn reply_snapshot(&self, cx: &App) -> Option<Draft> {
        self.conversations.active.as_ref().map(|target| {
            Draft::Reply(ReplyDraft {
                target: target.clone(),
                body: self.conversations.input.read(cx).value().to_string(),
            })
        })
    }
    pub(super) fn defer_reply_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.conversations.timer = None;
        self.conversations.deferred.clear();
        if let Some(draft) = self.reply_snapshot(cx) {
            let generation = self.conversations.deferred.replace(draft);
            self.save_status = "Saving local reply…".into();
            let timer = cx.background_executor().timer(DRAFT_QUIET_PERIOD);
            self.conversations.timer = Some(cx.spawn_in(window, async move |this, cx| {
                timer.await;
                let _ = this.update_in(cx, |this, window, cx| {
                    if let Some(draft) = this.conversations.deferred.take(generation) {
                        this.conversations.timer = None;
                        if !this.closed {
                            this.persist_draft(draft, window, cx);
                        }
                    }
                });
            }));
        }
        cx.notify();
    }
    pub(super) fn save_reply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.conversations.timer = None;
        self.conversations.deferred.clear();
        if let Some(draft) = self.reply_snapshot(cx) {
            self.persist_draft(draft, window, cx);
        }
    }
    pub(super) fn receive_threads(&mut self, page: CursorPage<Thread>, cursor: Option<String>) {
        self.conversations.invalid_threads.clear();
        self.conversations.threads = page.items;
        self.conversations.total = page.total_count;
        let had_next = page.next_cursor.is_some();
        self.conversations.next_cursor =
            accept_page_cursor(&mut self.conversations.cursors, cursor, page.next_cursor);
        self.conversations.pagination_notice = (had_next && self.conversations.next_cursor.is_none()).then(||"Further conversation pages are unavailable: the provider repeated a cursor or this inspection reached its 1,000-page limit. Refresh conversations to start a new inspection.".into());
        self.conversations.loaded = true;
    }
    fn invalidate_thread_context(&mut self, target: Option<&CapturedThread>) {
        for thread in &mut self.conversations.threads {
            if target.is_none_or(|target| &thread.target == target) {
                thread.viewer_can_reply = false;
                thread.viewer_can_resolve = false;
                thread.viewer_can_unresolve = false;
                self.conversations
                    .invalid_threads
                    .insert(thread.target.thread_id.clone());
            }
        }
        self.confirm = None;
    }
    fn receive_thread(&mut self, thread: Thread) {
        if let Some(old) = self
            .conversations
            .threads
            .iter_mut()
            .find(|old| old.target == thread.target)
        {
            self.conversations
                .invalid_threads
                .remove(&thread.target.thread_id);
            *old = thread;
        }
    }
    /// Says `notice`, which concerns the conversation `about` ([`State::notice`]).
    fn say_about(&mut self, notice: String, about: CapturedThread) {
        self.conversations.notice = Some((notice.clone(), about));
        self.notice = Some(notice);
    }
    /// Whether the panel's notice concerns the conversation `target`, as its last notice about
    /// a conversation did, rather than anything since.
    fn notice_is_about(&self, target: &CapturedThread) -> bool {
        matches!(
            (&self.notice, &self.conversations.notice),
            (Some(notice), Some((said, about))) if notice == said && about == target
        )
    }
    pub(super) fn restore_reply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let saved = self.saved.iter().find_map(|draft| match draft {
            Draft::Reply(reply)
                if !reply.body.is_empty()
                    && !drafts::reply_was_completed(&self.attempts, reply)
                    && self.conversations.threads.iter().any(|thread| {
                        reply_matches(thread, &reply.target, self.account.as_deref())
                    }) =>
            {
                Some(reply.clone())
            }
            _ => None,
        });
        if let Some(reply) = saved {
            self.conversations.active = Some(reply.target.clone());
            self.conversations
                .input
                .update(cx, |input, cx| input.set_value(reply.body, window, cx));
            self.say_about(
                "Unfinished reply restored with its exact conversation and account.".into(),
                reply.target,
            );
        }
    }
    fn thread_page(&mut self, cursor: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pull) = self.captured(cx) else {
            return;
        };
        self.save_draft(window, cx);
        let fixture = self.review.fixture;
        let moved = self.review.fixture_moved;
        let requested = cursor.clone();
        self.confirm = None;
        self.run(
            RequestKind::Load,
            move |control| {
                Client(UiTransport::open(fixture, moved)?).review_threads(
                    &pull,
                    requested.as_deref(),
                    &control,
                )
            },
            move |result, this, _, _| match result {
                Ok(page) => this.receive_threads(page, cursor),
                Err(error) => {
                    this.invalidate_thread_context(None);
                    this.error = Some(format!("Conversations could not be loaded: {error:#}. Cached conversation actions are disabled until a successful explicit refresh."))
                }
            },
            window,
            cx,
        );
    }
    fn thread_comments(
        &mut self,
        target: CapturedThread,
        cursor: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.save_draft(window, cx);
        let fixture = self.review.fixture;
        let moved = self.review.fixture_moved;
        let captured = target.clone();
        self.confirm = None;
        self.run(
            RequestKind::Load,
            move |control| {
                Client(UiTransport::open(fixture, moved)?).thread_comments(
                    &target,
                    cursor.as_deref(),
                    &control,
                )
            },
            move |result, this, _, _| match result {
                Ok(thread) => this.receive_thread(thread),
                Err(error) => {
                    this.invalidate_thread_context(Some(&captured));
                    this.error = Some(format!(
                        "Conversation context could not be loaded: {error:#}. This conversation’s actions are disabled until a successful explicit refresh."
                    ))
                }
            },
            window,
            cx,
        );
    }
    /// Opens `target`'s reply composer, focused on its draft. An outcome asked for from
    /// another conversation's composer stays that conversation's ([`Origin::Reply`]): it
    /// moves to the panel's top line, never under this draft. A notice from before that is
    /// not about this conversation is stale where the person now looks, and goes: pinned above
    /// the body while the reply is open ([`Panel::pin_top_line`]), it kept room the composer
    /// needs, and a refused reply's failure once fell below the body's clip under it.
    pub(super) fn begin_reply(
        &mut self,
        target: CapturedThread,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending
            || !self
                .conversations
                .threads
                .iter()
                .any(|thread| reply_matches(thread, &target, self.account.as_deref()))
        {
            self.refuse("This conversation cannot accept a reply in the captured account and PR context. Refresh conversations explicitly, or copy the saved draft.");
            cx.notify();
            return;
        }
        self.save_draft(window, cx);
        if self.conversations.active.as_ref() != Some(&target) {
            let body = self
                .saved
                .iter()
                .find_map(|draft| match draft {
                    Draft::Reply(reply)
                        if reply.target == target
                            && !drafts::reply_was_completed(&self.attempts, reply) =>
                    {
                        Some(reply.body.clone())
                    }
                    _ => None,
                })
                .unwrap_or_default();
            if !self.notice_is_about(&target) {
                self.notice = None;
            }
            self.conversations.active = Some(target);
            self.conversations
                .input
                .update(cx, |input, cx| input.set_value(body, window, cx));
        }
        self.confirm = None;
        *self.review.revealed_focus.borrow_mut() = Default::default();
        self.conversations
            .input
            .read(cx)
            .focus_handle(cx)
            .focus(window, cx);
        cx.notify();
    }
    pub(super) fn review_reply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_draft(window, cx);
        let Some(Draft::Reply(reply)) = self.reply_snapshot(cx) else {
            return;
        };
        // Review reply… is pressed in this conversation's reply composer, where its refusal
        // answers.
        let origin = Origin::Reply(Box::new(reply.target.clone()));
        if let Err(error) = github::validate_text(&reply.body, true) {
            self.refuse_at(origin, error.to_string());
            cx.notify();
            return;
        }
        if !self
            .conversations
            .threads
            .iter()
            .any(|thread| reply_matches(thread, &reply.target, self.account.as_deref()))
        {
            self.refuse_at(origin, "The captured reply context is unavailable or changed. Refresh this conversation explicitly; the original draft remains available to copy.");
            cx.notify();
            return;
        }
        self.confirm = Some(Confirmation {
            account: reply.target.account.clone(),
            action: Action::Reply {
                target: reply.target,
                body: reply.body,
            },
        });
        self.focus_confirmation(window, cx);
    }
    fn review_resolution(
        &mut self,
        target: CapturedThread,
        resolved: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.save_draft(window, cx);
        let valid = self.account.as_deref() == Some(target.account.as_str())
            && self.conversations.threads.iter().any(|thread| {
                thread.target == target
                    && if resolved {
                        !thread.is_resolved && thread.viewer_can_resolve
                    } else {
                        thread.is_resolved && thread.viewer_can_unresolve
                    }
            });
        if !valid || self.pending {
            self.refuse("GitHub has not granted this account permission to change this conversation in the captured context. Refresh conversations to check again.");
            cx.notify();
            return;
        }
        self.confirm = Some(Confirmation {
            account: target.account.clone(),
            action: Action::SetThreadResolved { target, resolved },
        });
        self.focus_confirmation(window, cx);
    }
    /// What the top line says in this frame ([`TopLineTexts`]), from the state the panel's
    /// render builds its lines from.
    fn pinned_line_texts(&self) -> TopLineTexts<'_> {
        if self.conversations.active.is_none() {
            return [None; 3];
        }
        [
            self.notice.as_deref(),
            self.error
                .as_deref()
                .or(self.cancelled)
                .filter(|_| !self.answers_in_reply()),
            self.draft_issue.map(DraftIssue::message),
        ]
    }
    /// What the top line's busy row says in this frame, where one stands there: while a
    /// conversation is open and a request runs.
    fn pinned_busy_label(&self) -> Option<&'static str> {
        (self.conversations.active.is_some() && self.pending).then(|| self.request.label())
    }
    pub(super) fn focus_confirmation(&self, window: &mut Window, cx: &mut Context<Self>) {
        *self.review.revealed_focus.borrow_mut() = Default::default();
        // A new confirmation shows its destination first.
        self.conversations
            .confirm_scroll
            .set_offset(point(px(0.), px(0.)));
        self.conversations.confirm_focus.focus(window, cx);
        cx.notify();
    }
    /// Scrolls the outbound confirmation's region, which holds focus, by `key`: a line up or
    /// down, a page, its view less a line, up or down, or to either end.
    pub(super) fn scroll_confirmation(&mut self, key: &str, cx: &mut Context<Self>) {
        let scroll = &self.conversations.confirm_scroll;
        let (offset, end) = (-scroll.offset().y, scroll.max_offset().y.max(Pixels::ZERO));
        let line = self.conversations.confirm_fit.get().line.max(px(1.));
        let page = (scroll.bounds().size.height - line).max(line);
        let to = match key {
            "up" => offset - line,
            "down" => offset + line,
            "pageup" => offset - page,
            "pagedown" => offset + page,
            "home" => Pixels::ZERO,
            _ => end,
        }
        .clamp(Pixels::ZERO, end);
        if to != offset {
            scroll.set_offset(point(scroll.offset().x, -to));
            cx.notify();
        }
    }
    /// The outbound confirmation. Its one Tab stop is a region that scrolls the destination,
    /// with every full identity it names, and then the text Send sends: the region is as tall
    /// as what it holds, up to the room the panel's body leaves beside the rest of the
    /// confirmation, and never less than two lines, so the region's ring and Send and Keep
    /// editing below it stay in view together where the body can hold them. Laid out by what it
    /// holds in each frame, a confirmation opens at its own height: sized from what the last one
    /// held, a short comment's opened as tall as the long reply's before it, and a new panel's
    /// took the whole room. The guidance sentence stands above Send where the body holds it, the
    /// region's two lines and the buttons, and otherwise in the region after the destination.
    /// Where it stands and the rest of the confirmation's height are decided before layout in
    /// the frame being drawn, from the panel's width and the interface font ([`ConfirmForm`]),
    /// so no frame draws the guidance where the next moves it from, and the body's reveal of
    /// what holds focus is decided from that frame's layout too, so a first frame or a resize
    /// that reflows the confirmation draws Send and Keep editing whole in the body's view
    /// where the body can hold them. Shown whole, the confirmation was the stop, 446 px tall at
    /// 18 pt in a 461 × 490 window,
    /// where the body is about 265 px tall, and no reveal could draw its ring whole. The region
    /// draws its ring on the edge of the room it keeps around itself, inside the confirmation's
    /// padding, whose resting border stays as it is, so nothing moves for the ring.
    /// `flowing` is where the top line lies among the content this frame, if it does
    /// ([`Panel::flow_top_line`]), set before the confirmation below it prepaints.
    pub(super) fn render_confirmation(
        &self,
        confirmation: &Confirmation,
        flowing: std::rc::Rc<std::cell::Cell<Option<(Pixels, Pixels)>>>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = palette(cx);
        let pending = self.pending;
        let drawn = self.conversations.confirm_fit.get();
        // The room around the region its ring lies on the edge of.
        let ring = appearance::button_ring_room(cx);
        let gap = window.pixel_snap(window.rem_size() * 0.5);
        let send = if self.review.fixture {
            "Send to offline fixture"
        } else {
            "Send to GitHub"
        };
        // One read of the text style for the frame, for the region's line and the form.
        let style = window.text_style();
        let line = confirm_line(&style, window);
        let (guidance, chrome, top_text, busy_text) = self.conversations.confirm_form(
            &style,
            send,
            self.pinned_line_texts(),
            self.pinned_busy_label(),
            window,
            cx,
        );
        // The body's view in this frame, less the room the confirmation's reveal keeps at either
        // edge: the room the dialog leaves the body in this window, less the top line standing
        // above the body while a conversation is open, as tall as what it says in this frame.
        // The dialog sets no height of its own and the line and the body share a wrapper capped
        // at the room, so an overflowing body's view is exactly that. Read from the view the
        // last frame drew, a window made only shorter, 1,480 × 973 to 1,480 × 486 at 13 pt,
        // which rewraps nothing and so measures nothing new, kept a long reply's region 612 px
        // tall in a 290 px body, Send below its foot, until some other frame drew. And read from
        // the height the last frame drew the top line, a resize that rewrapped a notice on it,
        // 1,480 to 461 px at 13 pt, laid the region out 39 px too tall, then 39 px too short,
        // and only in the third frame at its height.
        let pinned = self.conversations.active.is_some();
        let top_line = pinned_line_height(top_text, busy_text, window);
        let body_room = super::body_max_height(window);
        let room = window.pixel_snap(body_room - top_line);
        let left = room - super::review::reveal_room(window, cx) * 2.;
        let floor = line * 2.;
        let inside = chrome + guidance + gap + floor > left;
        // The tallest the region is: what it holds is laid out up to this in the same frame.
        let cap = (left - chrome - if inside { Pixels::ZERO } else { guidance + gap }).max(floor);
        let guidance = label("github-confirm-guidance", GUIDANCE).text_color(rgb(p.muted));
        let (within, below) = if inside {
            (Some(guidance), None)
        } else {
            (None, Some(guidance))
        };
        let text = match &confirmation.action {
            Action::Create(pull) => format!("{}\n\n{}", pull.title, pull.body),
            Action::Comment { body, .. } | Action::Reply { body, .. } => body.clone(),
            Action::SetThreadResolved { resolved, .. } => if *resolved {
                "Resolve this conversation"
            } else {
                "Reopen this conversation"
            }
            .into(),
            Action::Review(review) => format!(
                "{} review · {}\n\n{}{}",
                review.event.label(),
                crate::counted(review.comments.len(), "inline comment", "inline comments"),
                review.body,
                review
                    .comments
                    .iter()
                    .map(|comment| format!(
                        "\n\n{}\n{}",
                        github::review::position_label(comment),
                        comment.body
                    ))
                    .collect::<String>()
            ),
        };
        let focus = &self.conversations.confirm_focus;
        let edge = self
            .owner
            .upgrade()
            .and_then(|owner| owner.read(cx).focus_ring.edge(focus, rgb(p.accent)));
        let region = crate::native_accessibility::clips_children(
            div()
                .id("github-confirm-region")
                .debug_selector(|| "github-confirm-region".into())
                .key_context(super::review::CONFIRMATION)
                .on_action(cx.listener(|this, _: &super::review::ReviewUp, _, cx| {
                    this.scroll_confirmation("up", cx);
                    cx.stop_propagation();
                }))
                .on_action(cx.listener(|this, _: &super::review::ReviewDown, _, cx| {
                    this.scroll_confirmation("down", cx);
                    cx.stop_propagation();
                }))
                .on_action(cx.listener(|this, _: &super::review::ReviewHome, _, cx| {
                    this.scroll_confirmation("home", cx);
                    cx.stop_propagation();
                }))
                .on_action(cx.listener(|this, _: &super::review::ReviewEnd, _, cx| {
                    this.scroll_confirmation("end", cx);
                    cx.stop_propagation();
                }))
                .on_action(cx.listener(|this, _: &super::review::ReviewPageUp, _, cx| {
                    this.scroll_confirmation("pageup", cx);
                    cx.stop_propagation();
                }))
                .on_action(
                    cx.listener(|this, _: &super::review::ReviewPageDown, _, cx| {
                        this.scroll_confirmation("pagedown", cx);
                        cx.stop_propagation();
                    }),
                )
                .track_focus(focus)
                .track_scroll(&self.conversations.confirm_scroll)
                .role(Role::Group)
                .aria_label(format!(
                    "{}: its destination and what Send sends",
                    confirmation_title(&confirmation.action)
                ))
                // As tall as what it holds, up to the cap: it and the confirmation do not
                // shrink, so the body's column lays both out at what they hold, and the body
                // scrolls to the region's foot and to Send. A scrolling element adds nothing
                // of what it holds to a column that shrinks it, so laid out in a confirmation
                // that could shrink, the region drew past the confirmation's laid-out foot.
                .flex_shrink_0()
                .min_w_0()
                .max_h(cap)
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex_shrink_0()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            label(
                                "github-confirm-destination",
                                format!(
                                    "{}\nAccount · {}",
                                    confirmation.action.destination(),
                                    confirmation.account
                                ),
                            )
                            .font_weight(FontWeight::SEMIBOLD),
                        )
                        .children(within)
                        .child(label("github-confirm-text", text)),
                ),
        );
        // What the body's view gave the confirmation, read as it prepaints, and the line the
        // region's keys move it by.
        let (fit, revealed, held) = (
            self.conversations.confirm_fit.clone(),
            self.review.revealed_focus.clone(),
            focus.clone(),
        );
        let body_scroll = self.review.body_scroll.clone();
        // Each frame as it prepaints, for tests ([`ConfirmFrame`]).
        #[cfg(test)]
        let record = {
            let body_scroll = body_scroll.clone();
            move |children: &[Bounds<Pixels>]| {
                let (Some(around), Some(span)) = (
                    children.first(),
                    children.iter().copied().reduce(|a, b| a.union(&b)),
                ) else {
                    return;
                };
                let below = match children {
                    [_, guidance, _] => guidance.size.height + gap,
                    _ => Pixels::ZERO,
                };
                CONFIRM_FRAMES.with(|frames| {
                    frames.borrow_mut().push(ConfirmFrame {
                        ring: *around,
                        actions: children[children.len() - 1],
                        body: body_scroll.bounds(),
                        inside: children.len() == 2,
                        chrome: span.size.height - (around.size.height - ring * 2.) - below,
                    })
                });
            }
        };
        let stood = self.review.top_line.clone();
        let measure = move |children: &[Bounds<Pixels>], window: &Window| {
            // The top line above the body prepainted its height before the body, which laid out
            // and prepainted its own bounds before what it holds prepaints.
            let line_left = if pinned { stood.get() } else { Pixels::ZERO };
            let line_among = flowing
                .get()
                .map_or(Pixels::ZERO, |(top, bottom)| bottom - top);
            // The region's ring, the first of the children, and the buttons, the last.
            let view = body_scroll.bounds();
            let in_view = [children.first(), children.last()]
                .into_iter()
                .flatten()
                .any(|part| part.top() < view.bottom() && part.bottom() > view.top());
            fit.set(ConfirmFit {
                line,
                top_line: line_left,
                line_among,
                in_view,
            });
            // The line changed height: this frame has sized the region for it, and the line's
            // shift of the body's scroll left Send and Keep editing where the change put them,
            // so where the region holds focus the reveal just below shows it as it lies now.
            // Without it, a line growing by more than the room above the region's ring left the
            // ring's top under the body's new top, 18 px at 13 pt in a 1,480 × 973 window.
            // The line among the content moves the region as well, with no such shift: Refresh
            // PRs pressed with the pointer, focus staying on the region, drew its busy row above
            // the confirmation, 63 px with the row wrapped, and Send stayed below the body's foot.
            // A body wheeled away from the confirmation stays where it is, whichever line moved:
            // revealed, it left the row and the Cancel a press of Refresh PRs asked for, and,
            // with a conversation open, a notice growing above the body or wrapping as the
            // window narrowed pulled the body back from what the wheel had shown.
            let changed = |now: Pixels, then: Pixels| (now - then).abs() > px(0.5);
            if (changed(line_left, drawn.top_line) || changed(line_among, drawn.line_among))
                && drawn.in_view
                && held.is_focused(window)
            {
                *revealed.borrow_mut() = Default::default();
            }
        };
        let reveal = self.reveal_ringed_on_focus(focus.clone());
        // Laid out before any of the confirmation's children prepaint, so the frame that first
        // draws it reveals it whole ([`super::review::Reopening::reveal_laid_out`]).
        let mark = self.confirmation_mark(focus, super::review::reveal_room(window, cx));
        div()
            .debug_selector(|| "github-confirmation".into())
            .flex_shrink_0()
            .on_children_prepainted(move |mut children, window, cx| {
                // The mark over the confirmation is none of what its reveals show.
                children.remove(0);
                #[cfg(test)]
                record(&children);
                measure(&children, window);
                reveal(children, window, cx);
            })
            .map(super::bordered_pane)
            .border_1()
            .border_color(rgb(p.border))
            .rounded(px(7.))
            .flex()
            .flex_col()
            .gap_2()
            .child(mark)
            .child(
                div()
                    .debug_selector(|| "github-confirm-ring".into())
                    .relative()
                    .flex_shrink_0()
                    .min_w_0()
                    // The room reaches into the confirmation's side padding, so the text keeps
                    // the panel's text edge; above and below it is the room's own. Taken back by
                    // a negative margin there too, the room left the confirmation laid out as
                    // tall as its padding alone, 21 px at 13 pt, everything in it drawn below
                    // its foot, and the body, laid out without it, could not scroll to it.
                    .mx(-ring)
                    .p(ring)
                    .child(super::review::clipped(region))
                    // Over the region's text and under its ring ([`region_fades`]).
                    .child(region_fades(self.conversations.confirm_scroll.clone()))
                    .children(edge),
            )
            .children(below)
            .child(
                // Two lines where one is too narrow for both, as at 18 pt in a 461 px window,
                // where Keep editing ran past the body's edge.
                self.part("confirmation-actions", div(), cx)
                    .child(self.confirmation_actions_mark(cx))
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        self.button("github-confirm-send", send, "", true, cx)
                            .debug_selector(|| "github-confirm-send".into())
                            .disabled(pending)
                            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                    )
                    .child(
                        self.button("github-edit-submission", "Keep editing", "", false, cx)
                            .debug_selector(|| "github-edit-submission".into())
                            .disabled(pending)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.confirm = None;
                                this.focus_visible_section(window, cx);
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }
    pub(super) fn finish_conversation_action(
        &mut self,
        action: &Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            Action::Reply { target, body } => {
                // The same coalesced saver replaces any queued pre-send snapshot.
                // Exact text/target matching preserves edits made after capture.
                let matching = matches!(self.reply_snapshot(cx), Some(Draft::Reply(reply)) if reply.target == *target && reply.body == *body);
                if matching {
                    self.conversations.timer = None;
                    self.conversations.deferred.clear();
                    self.conversations.active = None;
                    self.conversations
                        .input
                        .update(cx, |input, cx| input.set_value("", window, cx));
                }
                let newer = matches!(self.reply_snapshot(cx),Some(Draft::Reply(reply)) if reply.target == *target && reply.body != *body && !reply.body.is_empty()) || self.saved.iter().any(|draft| matches!(draft,Draft::Reply(reply) if reply.target == *target && reply.body != *body && !reply.body.is_empty()));
                if matching || !newer {
                    self.persist_draft(
                        Draft::Reply(ReplyDraft {
                            target: target.clone(),
                            body: String::new(),
                        }),
                        window,
                        cx,
                    );
                }
                self.say_about(
                    "Reply sent. Refresh this conversation to read GitHub’s latest comments."
                        .into(),
                    target.clone(),
                );
                self.focus_visible_section(window, cx);
            }
            Action::SetThreadResolved { target, resolved } => {
                if let Some(thread) = self
                    .conversations
                    .threads
                    .iter_mut()
                    .find(|thread| thread.target == *target)
                {
                    thread.is_resolved = *resolved;
                    // Permissions must be obtained again from GitHub before another mutation.
                    thread.viewer_can_resolve = false;
                    thread.viewer_can_unresolve = false;
                }
                self.say_about(
                    format!(
                        "Conversation {}. Refresh conversations to update available actions.",
                        if *resolved { "resolved" } else { "reopened" }
                    ),
                    target.clone(),
                );
                self.review.section_focus.focus(window, cx);
            }
            _ => {}
        }
    }
    /// For the element holding the open composer's draft and the outcome under it: as an
    /// outcome appears there, or a press is answered again, the body scrolls by the least that
    /// shows the draft with it, a line's room clear of its edges, wherever they are not whole
    /// in view, and focus stays where it is: the composer says why where the person is
    /// looking. A refused reply's failure once stood below the body's clip, at 13 pt in a
    /// 461 × 490 window under a notice pinned above the body, with nothing to show it.
    fn reveal_outcome(&self) -> impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static {
        // Only an outcome the composer draws: a request starting from the composer, as Send
        // does, clears the last one and has nothing new to show, and once scrolled the body
        // back to the draft, away from what the person had just pressed. Recorded as nothing,
        // the next outcome to appear is new.
        let outcome = (self.answers_in_reply()
            && (self.error.is_some() || self.cancelled.is_some()))
        .then(|| (self.error.clone(), self.cancelled, self.presses.error));
        let shown = self.conversations.outcome_shown.clone();
        let scroll = self.review.body_scroll.clone();
        let blocks = self.review.body_blocks.clone();
        move |bounds, window, cx| {
            if *shown.borrow() == outcome {
                return;
            }
            shown.replace(outcome.clone());
            if outcome.is_some() {
                let inset = window.rem_size() + px(2.);
                let still = |_: &Window, _: &App| true;
                super::review::show(&scroll, &blocks, bounds, inset, false, still, window, cx);
            }
        }
    }
    pub(super) fn render_reply_composer(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(target) = self.conversations.active.as_ref() else {
            return div().into_any_element();
        };
        let p = palette(cx);
        let valid = self
            .conversations
            .threads
            .iter()
            .any(|thread| reply_matches(thread, target, self.account.as_deref()));
        div().debug_selector(||"github-reply-composer".into()).on_children_prepainted(self.reveal_on_focus(self.conversations.input.read(cx).focus_handle(cx)))
            .child(self.stop_mark(&self.conversations.input.read(cx).focus_handle(cx),super::review::reveal_room,false,true))
            .min_w_0().map(super::bordered_pane).rounded(px(7.)).border_1().border_color(rgb(p.border)).bg(rgb(p.subtle)).flex().flex_col().gap_2()
            .child(label("github-reply-heading", format!("Reply to conversation · {}", target.path)).font_weight(FontWeight::SEMIBOLD))
            .child(label("github-reply-context", format!("{} · #{} · {} · captured head {}",target.account,target.pull.number,target.pull.repository.label(),target.pull.head.sha)).text_color(rgb(p.muted)))
            .when(!valid,|element|element.child(label("github-reply-unavailable","Original context is unavailable on this page. Refresh or return to its conversation before sending. Your exact draft remains local.").text_color(rgb(p.warning))))
            // The draft and the outcome under it, which the body shows together as it appears.
            .child(div().debug_selector(||"github-reply-draft".into()).flex().flex_col().gap_2().on_children_prepainted(self.reveal_outcome())
                .child(div().child(super::review::editor_rows(&self.conversations.input)).child(Textarea::new(&self.conversations.input).h(appearance::ui_size(100.)).aria_label("Reply to captured GitHub conversation, saved locally").disabled(self.pending)))
                .when_some(self.outcome("github-reply-error","github-reply-cancelled",cx).filter(|_|self.answers_in_reply()),|element,line|element.child(super::review::clipped(div().id("github-reply-error-scroll").debug_selector(||"github-reply-outcome".into()).flex_shrink_0().max_h(appearance::ui_size(84.)).overflow_y_scroll().child(line)))))
            .child(div().flex().flex_wrap().items_center().gap_2()
                .child(self.button("github-review-reply","Review reply…","",true, cx).disabled(self.pending||!valid).on_click(cx.listener(|this,_,window,cx|this.review_reply(window,cx))))
                .child(self.button("github-close-reply","Save and close reply","",false, cx).disabled(self.pending).on_click(cx.listener(|this,_,window,cx|{this.save_reply(window,cx);this.conversations.active=None;this.confirm=None;this.focus_visible_section(window,cx);cx.notify();})))
                .child(self.held_button("github-copy-reply",copy_button(reply_mark(target),"Copy reply", "", cx),cx).on_click(cx.listener(|this,_,window,cx|{if let Some(draft)=this.reply_snapshot(cx){let Draft::Reply(reply)=&draft else {return};copy_feedback::copy(window,cx,Some(reply_mark(&reply.target)),"reply",draft.recovery_text());}})))
                .child(label("github-reply-save",self.save_status.clone()).role(Role::Status).text_color(rgb(p.muted))))
            .into_any_element()
    }
    pub(super) fn render_threads(
        &self,
        path: Option<&str>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = palette(cx);
        // The pane, its own focus ring on its edge, fits inside the panel's body with the
        // room the body keeps for rings, so Tab shows it whole: its list takes what the
        // body has beyond the rest of the pane, up to its own bound.
        let room = appearance::button_ring_room(cx);
        let chrome = self.conversations.list_chrome.clone();
        let list_height = appearance::ui_size(360.).min(
            (super::body_max_height(window) - room * 2. - chrome.get())
                .max(appearance::ui_size(120.)),
        );
        let list_scroll = self.conversations.list_scroll.clone();
        let list_revealed = self.conversations.list_revealed.clone();
        let list_blocks = self.conversations.list_blocks.clone();
        let body_blocks = self.review.body_blocks.clone();
        let visible = self
            .conversations
            .threads
            .iter()
            .filter(|thread| path.is_none_or(|path| thread.target.path == path))
            .collect::<Vec<_>>();
        let unresolved = visible.iter().filter(|thread| !thread.is_resolved).count();
        let previous = self
            .conversations
            .cursors
            .len()
            .checked_sub(2)
            .map(|index| self.conversations.cursors[index].clone());
        let next = self.conversations.next_cursor.clone();
        // The pane is a part of the body, and so are its header, its list and its pager:
        // Tab into one shows that one, and onto the pane itself shows the whole pane.
        self.ringed_part("conversations", div(), &self.review.section_focus, cx).child(self.stop_mark(&self.review.section_focus,super::review::ring_room,true,false)).min_w_0().child(crate::native_accessibility::clips_children(div()
            .on_children_prepainted(move |bounds, window, cx| {
                // Everything but the list, the second child: the pane's padding and border
                // around the span of its children, less the list.
                body_blocks.report(&bounds);
                let (Some(list), Some(span)) = (bounds.get(1).copied(), bounds.iter().copied().reduce(|a, b| a.union(&b))) else { return };
                let measured = span.size.height - list.size.height + window.rem_size() * 1.5 + px(2.);
                if (measured - chrome.get()).abs() > px(0.5) {
                    chrome.set(measured);
                    window.defer(cx, |window, _| window.refresh());
                }
            })
            .id("github-thread-section").debug_selector(||"github-thread-section".into()).min_w_0().role(Role::Group).aria_label("GitHub review conversations with authoritative resolution status")
            .track_focus(&self.review.section_focus).border_1().border_color(rgb(if self.ringed(&self.review.section_focus, cx) { p.accent } else { p.border })).rounded(px(7.)).flex().flex_col().gap_2()
            // Its border and side padding together are the panel's text inset, so its heading
            // and conversations start at the panel's text edge with its frame at the frames'
            // column; 0.75 rem inside the border, they started 0.65 px past it at 18 pt.
            .map(super::bordered_pane)
            .child(self.part("conversations-header", div(), cx).flex().flex_wrap().items_center().gap_2()
                .child(super::phrases("github-conversations-heading",format!("Conversations · {} unresolved on this page",unresolved)).min_w_0().font_weight(FontWeight::SEMIBOLD))
                .child(div().flex_1())
                .child(self.button("github-threads-refresh","Refresh conversations","refresh",false, cx).disabled(self.pending).on_click(cx.listener(|this,_,window,cx|this.thread_page(None,window,cx)))))
            .child(self.part("conversation-list", div(), cx).flex_shrink_0().min_w_0().child(super::review::clipped_lines(crate::native_accessibility::clips_children(div()
                .on_children_prepainted(move |_, window, cx| {
                    super::review::record_shown(&list_revealed, &list_scroll, |context| context.get(super::review::PART).is_some_and(|part| part == "conversation-list"), window, cx);
                    list_blocks.finish();
                })
                .id("github-threads").debug_selector(||"github-threads".into())
                .track_scroll(&self.conversations.list_scroll).flex_shrink_0().min_w_0().max_h(list_height).overflow_y_scroll().flex().flex_col().gap_3()
                .when(visible.is_empty(),|element|element.child(label("github-no-threads",if self.conversations.loaded{"No conversations for this file on the loaded page."}else{"Conversations are unavailable. Refresh conversations to load their current status."}).text_color(rgb(p.muted))))
                .children(visible.into_iter().enumerate().map(|(index,thread)|{
                    let target=thread.target.clone(); let reply=target.clone(); let resolve=target.clone(); let refresh=target.clone(); let more=target.clone();
                    let resolved=thread.is_resolved; let stale=self.conversations.invalid_threads.contains(&target.thread_id); let cursor=thread.next_comments_cursor.clone();
                    let opening_missing=target.root_comment_id.as_ref().is_some_and(|root|!thread.comments.iter().any(|comment|&comment.id==root));
                    let can_reply=reply_matches(thread,&target,self.account.as_deref());
                    let can_resolve=if resolved{thread.viewer_can_unresolve}else{thread.viewer_can_resolve};
                    div().on_children_prepainted(self.conversations.list_blocks.starts()).id(("github-conversation",index)).flex_shrink_0().min_w_0().p_3().bg(rgb(p.panel)).border_1().border_color(rgb(p.border)).rounded(px(6.)).flex().flex_col().gap_2()
                        .child(div().flex().flex_wrap().items_center().gap_2()
                            .child(label(("github-thread-path",index),thread.location()).font_weight(FontWeight::SEMIBOLD))
                            .child(div().flex_1())
                            .child(label(("github-thread-status",index),if stale{"Status needs refresh"}else if resolved{"Resolved"}else{"Unresolved"}).px_2().py_1().rounded(px(4.)).bg(rgb(p.subtle)).text_color(rgb(if resolved{p.muted}else{p.accent}))))
                        .when(stale,|element|element.child(label(("github-thread-stale",index),"The last refresh could not validate this conversation. Comments and drafts are retained; refresh successfully before replying or changing its status.").text_color(rgb(p.warning))))
                        .when(thread.is_outdated,|element|element.child(label(("github-thread-outdated",index),format!("Outdated context · original commit {}",target.original_commit_id.as_deref().unwrap_or("unavailable"))).text_color(rgb(p.muted))))
                        .when(opening_missing && !thread.context_incomplete,|element|element.child(label(("github-thread-earlier",index),"Earlier comments are on a previous page. Opening comments returns to the start of this exact conversation.").text_color(rgb(p.muted))))
                        .when(thread.context_incomplete,|element|element.child(label(("github-thread-incomplete",index),"Opening or earlier context is unavailable on this page. Replies keep their original conversation identity.").text_color(rgb(p.warning))))
                        .children(thread.comments.iter().enumerate().map(|(comment_index,comment)|{
                            let is_reply=comment.reply_to.is_some() || comment_index>0;
                            div().on_children_prepainted(self.conversations.list_blocks.starts()).min_w_0().when(is_reply,|element|element.ml_3().pl_3().border_l_1().border_color(rgb(p.border))).flex().flex_col().gap_1().py_1()
                                .child(div().flex().flex_wrap().items_center().gap_2()
                                    .child(label(("github-conversation-author",index * 1000 + comment_index),comment.author.clone()).font_weight(FontWeight::SEMIBOLD))
                                    .child(label(("github-conversation-time",index * 1000 + comment_index),format!("{}{}",comment.created_at.replace('T'," ").trim_end_matches('Z'),if is_reply{" · reply"}else{""})).text_color(rgb(p.muted))))
                                .child(label(("github-conversation-body",index * 1000 + comment_index),comment.body.clone()).min_w_0())
                        }))
                        .child(self.conversation_part(index, div(), cx).debug_selector(move||format!("github-thread-actions-{index}")).flex().flex_wrap().items_center().gap_2().pt_1()
                            .child(self.button(("github-thread-reply",index),"Reply","",false, cx).debug_selector(move||format!("github-thread-reply-{index}")).disabled(self.pending||!can_reply).on_click(cx.listener(move|this,_,window,cx|this.begin_reply(reply.clone(),window,cx))))
                            .child(self.button(("github-thread-resolution",index),if resolved{"Reopen"}else{"Resolve"},"",false, cx).disabled(self.pending||!can_resolve).on_click(cx.listener(move|this,_,window,cx|this.review_resolution(resolve.clone(),!resolved,window,cx))))
                            .child(self.button(("github-thread-reload",index),if opening_missing{"Opening comments"}else{"Refresh thread"},"refresh",false, cx).disabled(self.pending).on_click(cx.listener(move|this,_,window,cx|this.thread_comments(refresh.clone(),None,window,cx))))
                            .when(cursor.is_some(),|element|element.child(self.button(("github-thread-more",index),"Next comment page","",false, cx).disabled(self.pending).on_click(cx.listener(move|this,_,window,cx|this.thread_comments(more.clone(),cursor.clone(),window,cx)))))
                            .child(label(("github-thread-count",index),comments_shown(thread.comments.len(),thread.comments_total)).text_color(rgb(p.muted))))
                        .when(!stale && (!can_reply || !can_resolve),|element|element.child(label(("github-thread-permissions",index),"Unavailable actions need complete context and GitHub permission; refresh to check again.").text_color(rgb(p.muted))))
                }))), &self.conversations.list_blocks)))
            .when_some(self.conversations.pagination_notice.as_ref(),|element,notice|element.child(label("github-thread-pagination-limit",notice.clone()).text_color(rgb(p.warning))))
            .child(self.part("conversations-pager", div(), cx).flex().flex_wrap().items_center().gap_2()
                .child(self.button("github-threads-previous","Previous conversations","",false, cx).disabled(self.pending||previous.is_none()).on_click(cx.listener(move|this,_,window,cx|{if let Some(cursor)=previous.clone(){this.thread_page(cursor,window,cx);}})))
                .child(super::phrases("github-threads-page",format!("Page {} · {} total conversations",self.conversations.cursors.len(),self.conversations.total)).min_w_0().text_color(rgb(p.muted)))
                .child(self.button("github-threads-next","Next conversations","",false, cx).disabled(self.pending||next.is_none()).on_click(cx.listener(move|this,_,window,cx|this.thread_page(next.clone(),window,cx))))))).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::component::Root;
    use std::{cell::RefCell, rc::Rc};

    #[test]
    fn a_thread_counts_its_comments_with_their_noun() {
        assert_eq!(comments_shown(1, 1), "1 of 1 comment");
        assert_eq!(comments_shown(2, 5), "2 of 5 comments");
    }

    #[test]
    fn conversation_paging_keeps_forward_navigation_and_reports_real_bounds() {
        let mut history = vec![None];
        assert_eq!(
            accept_page_cursor(&mut history, None, Some("second".into())),
            Some("second".into())
        );
        assert_eq!(
            accept_page_cursor(&mut history, Some("second".into()), Some("third".into())),
            Some("third".into())
        );
        assert_eq!(
            accept_page_cursor(&mut history, None, Some("second".into())),
            Some("second".into()),
            "Previous must retain the legitimate next page"
        );
        assert_eq!(
            accept_page_cursor(&mut history, Some("second".into()), Some("second".into())),
            None,
            "a repeated current cursor is incomplete, not exhausted"
        );
        let mut history = (0..999)
            .map(|n| Some(format!("page-{n}")))
            .collect::<Vec<_>>();
        assert_eq!(
            accept_page_cursor(
                &mut history,
                Some("page-999".into()),
                Some("page-1000".into())
            ),
            None
        );
        assert_eq!(history.len(), 1000);
    }

    #[gpui::test]
    async fn reply_focus_navigation_recovery_and_exact_completion(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let fixture = tempfile::tempdir().unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["-c", "init.templateDir=", "init", "--initial-branch=main"])
                .arg(fixture.path())
                .output()
                .unwrap()
                .status
                .success()
        );
        crate::views::tests::fixture_identity(fixture.path());
        let repo = GitRepository::open(fixture.path()).unwrap();
        let panel_repo = repo.clone();
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let output = captured.clone();
        crate::views::tests::init_app(cx);
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
                app.page = AppPage::Repository;
                app.path = Some(repo.path().to_owned());
                app.repository = Some(repo);
                app
            });
            *output.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        cx.simulate_resize(size(px(1000.), px(680.)));
        let app = captured.borrow().as_ref().unwrap().clone();
        let panel = cx.update(|window, cx| {
            cx.new(|cx| Panel::new(app.downgrade(), panel_repo, "main".into(), window, cx))
        });
        app.read_with(cx, |app, _| app.operations.submit(|| Ok(())))
            .await
            .unwrap()
            .unwrap();
        cx.executor().run_until_parked();
        let pull = github::review::fixture_pull(false);
        let identity = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let page = Client(UiTransport::Fixture { moved: false })
            .review_threads(&identity, None, &OperationControl::default())
            .unwrap();
        let target = page.items[0].target.clone();
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.destination.update(cx, |input, cx| {
                    input.set_value("gitturtle-fixture/native-review", window, cx)
                });
                panel.account = Some("offline-reviewer".into());
                panel.pull = Some(pull);
                panel.review.show_list = false;
                panel.receive_threads(page, None);
                panel.close(window, cx);
            });
            app.update(cx, |app, cx| app.open_github(window, cx));
        });
        cx.executor().run_until_parked();
        let normal_font=cx.update(|_,cx| {
            let normal=gpui_kit::component::Theme::global_mut(cx).font_size;
            gpui_kit::component::Theme::global_mut(cx).font_size=px(17.);
            panel.update(cx,|panel,cx| {
                panel.rows=vec![github::review::fixture_pull(true)];
                panel.review.show_list=true;
                panel.notice=Some("PR list refreshed. The selected PR keeps its captured head until explicitly reopened.".into());
                cx.notify();
            });
            normal
        });
        for _ in 0..3 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
        }
        // The refreshed row stays whole in the list, its title and its context line, rather
        // than a list squeezed beside the retained detail.
        let number = github::review::fixture_pull(true).number;
        let list = cx.debug_bounds("github-pr-list").unwrap();
        let title = cx
            .debug_bounds(format!("github-pr-{number}").leak())
            .unwrap();
        let context = cx
            .debug_bounds(format!("github-pr-context-{number}").leak())
            .unwrap();
        assert!(
            context.size.height > px(0.)
                && title.top() >= list.top() - px(0.01)
                && context.bottom() <= list.bottom() + px(0.01),
            "refreshed PR rows must remain visible beside retained PR detail at enlarged text \
             size: {title:?} and {context:?} in {list:?}"
        );
        let (saved_before, attempts_before, density_before) = cx.update(|_, cx| {
            let density = app.read(cx).settings.density;
            app.update(cx, |app, _| {
                app.settings.density = appearance::Density::Compact
            });
            panel.update(cx, |panel, cx| {
                let previous = (panel.saved.clone(), panel.attempts.clone(), density);
                panel.saved = (0..6)
                    .map(|index| {
                        let mut captured = target.clone();
                        captured.thread_id = format!("recovery-geometry-{index}");
                        Draft::Reply(ReplyDraft {
                            target: captured,
                            body: format!(
                                "Exact old-head reply {index}\nPreserve this second line."
                            ),
                        })
                    })
                    .collect();
                panel.attempts = (0..5)
                    .map(|index| drafts::Attempt {
                        destination: format!(
                            "GitHub fixture conversation {index} · captured head {}",
                            target.pull.head.sha
                        ),
                        outcome: "Previous outbound result retained for explicit verification"
                            .into(),
                        completed_reply_sha256: None,
                    })
                    .collect();
                panel.review.recovery_open = true;
                cx.notify();
                previous
            })
        });
        for font in [17., 15.] {
            cx.update(|_, cx| {
                gpui_kit::component::Theme::global_mut(cx).font_size = px(font);
                panel.update(cx, |_, cx| cx.notify());
            });
            for _ in 0..3 {
                cx.update(|window, cx| window.draw(cx).clear(cx));
                cx.run_until_parked();
            }
            let saved = cx
                .debug_bounds("github-saved-drafts")
                .expect("rendered recovery rows");
            let attempts = cx
                .debug_bounds("github-attempt-list")
                .expect("rendered attempt rows");
            assert!(
                saved.size.height >= px(100.) && saved.size.height <= px(160.),
                "six recovery rows need a visible bounded region at {font}px: {saved:?}"
            );
            assert!(
                attempts.size.height >= px(70.) && attempts.size.height <= px(100.),
                "five attempt rows need a visible bounded region at {font}px: {attempts:?}"
            );
            assert!(
                cx.debug_bounds("github-threads").unwrap().size.height >= px(100.),
                "conversation region must also resist sibling pressure"
            );
        }
        cx.update(|_, cx| {
            app.update(cx, |app, _| app.settings.density = density_before);
            panel.update(cx, |panel, cx| {
                panel.saved = saved_before;
                panel.attempts = attempts_before;
                panel.review.recovery_open = false;
                cx.notify();
            });
        });
        cx.update(|window, cx| {
            gpui_kit::component::Theme::global_mut(cx).font_size = normal_font;
            panel.update(cx, |panel, cx| {
                panel.review.show_list = false;
                panel.notice = None;
                panel.begin_reply(target.clone(), window, cx);
            });
        });
        cx.simulate_input("Immediate reply café");
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                assert!(
                    panel
                        .conversations
                        .input
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(window)
                );
                assert_eq!(
                    panel.conversations.input.read(cx).value(),
                    "Immediate reply café"
                );
                panel.conversations.input.update(cx, |input, cx| {
                    input.set_value(" Exact reply\r\n  café\n", window, cx);
                    cx.emit(InputEvent::Change);
                });
                panel.body.update(cx, |input, cx| {
                    input.set_value("Independent summary", window, cx);
                    cx.emit(InputEvent::Change);
                });
                panel.review.composing = Some(github::LineComment {
                    path: target.path.clone(),
                    body: "Inline still here".into(),
                    line: 2,
                    side: github::DiffSide::Right,
                    start_line: None,
                    start_side: None,
                });
                panel.review.input.update(cx, |input, cx| {
                    input.set_value("Inline still here", window, cx)
                });
            })
        });
        // All three prose composers participate in the native focus chain.
        let editors = cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.review.section = Section::Review;
                cx.notify();
                panel.focus_visible_section(window, cx);
                [
                    panel.conversations.input.clone(),
                    panel.review.input.clone(),
                    panel.body.clone(),
                ]
            })
        });
        for editor in editors {
            let before = cx.update(|window, cx| {
                editor.read(cx).focus_handle(cx).focus(window, cx);
                editor.read(cx).value().to_string()
            });
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.simulate_keystrokes("tab");
            cx.update(|window, cx| {
                assert!(
                    !editor.read(cx).focus_handle(cx).is_focused(window),
                    "Tab must leave a prose composer for its next control"
                );
                assert_eq!(
                    editor.read(cx).value(),
                    before,
                    "Tab must not insert whitespace into prose"
                );
            });
            cx.simulate_keystrokes("shift-tab");
            cx.update(|window, cx| {
                assert!(
                    editor.read(cx).focus_handle(cx).is_focused(window),
                    "Shift+Tab returns from the adjacent control to its composer"
                );
                assert_eq!(editor.read(cx).value(), before);
            });
        }
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.review.section = Section::Overview;
                panel.focus_visible_section(window, cx);
                cx.notify();
            })
        });
        for _ in 0..3 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
        }
        let composer = cx.debug_bounds("github-reply-composer").unwrap();
        let viewport = cx.read(|cx| panel.read(cx).review.body_scroll.bounds());
        assert!(
            composer.size.height > px(100.)
                && composer.top() >= viewport.top()
                && composer.bottom() <= viewport.bottom(),
            "reply composer {composer:?} should fit inside {viewport:?}"
        );
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.save_draft(window, cx);
                panel.review.section = Section::Files;
                panel.review.mode = 2;
                panel.close(window, cx);
            })
        });
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
        cx.executor().run_until_parked();
        cx.update(|window,cx|panel.update(cx,|panel,cx| {
            assert_eq!(panel.conversations.active.as_ref(),Some(&target));
            assert_eq!(panel.conversations.input.read(cx).value()," Exact reply\r\n  café\n");
            assert_eq!(panel.body.read(cx).value(),"Independent summary");
            assert!(panel.conversations.input.read(cx).focus_handle(cx).is_focused(window));
            panel.review_reply(window,cx);
            assert!(matches!(panel.confirm.as_ref().map(|c|&c.action),Some(Action::Reply { target:captured,.. }) if captured==&target));
            assert!(panel.conversations.confirm_focus.is_focused(window));
        }));
        for _ in 0..3 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
        }
        // Root supplies the real Dialog Return binding. Enter on a focused
        // reply button must run the button, never the dialog's default close.
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.confirm = None;
                panel.focus_visible_section(window, cx);
                cx.notify();
            })
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.simulate_keystrokes("tab");
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let enter = gpui_kit::Keystroke::parse("enter").unwrap();
        cx.simulate_event(gpui_kit::KeyDownEvent {
            keystroke: enter.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(gpui_kit::KeyUpEvent { keystroke: enter });
        cx.update(|window,cx| panel.update(cx,|panel,cx| {
            assert!(window.has_active_dialog(cx), "Return must not implicitly close the PR dialog");
            assert!(!panel.closed);
            assert!(matches!(panel.confirm.as_ref().map(|confirmation|&confirmation.action),Some(Action::Reply {target:captured,..}) if captured==&target));
        }));
        for _ in 0..3 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
        }
        let confirmation = cx.debug_bounds("github-confirmation").unwrap();
        let viewport = cx.read(|cx| panel.read(cx).review.body_scroll.bounds());
        assert!(
            confirmation.bottom() <= viewport.bottom(),
            "confirmation must reveal its send action"
        );
        // A provider refusal must be readable beside the still-focused draft,
        // even when the outer viewport was scrolled to the reply before sending.
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.review.fixture = true;
                panel.review.fixture_moved = true;
                panel.submit(window, cx);
            })
        });
        app.read_with(cx, |app, _| app.operations.submit(|| Ok(())))
            .await
            .unwrap()
            .unwrap();
        cx.executor().run_until_parked();
        cx.update(|_, cx| {
            panel.update(cx, |panel, cx| {
                assert!(
                    panel.error.is_some(),
                    "stale fixture head refuses the actual captured outbound reply"
                );
                assert_eq!(
                    panel.conversations.input.read(cx).value(),
                    " Exact reply\r\n  café\n"
                );
            })
        });
        for _ in 0..3 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
        }
        let outcome = cx
            .debug_bounds("github-reply-outcome")
            .expect("visible reply refusal");
        let viewport = cx.read(|cx| panel.read(cx).review.body_scroll.bounds());
        assert!(
            outcome.top() >= viewport.top() && outcome.bottom() <= viewport.bottom(),
            "reply refusal {outcome:?} stays visible beside its draft in {viewport:?}"
        );
        // A status, no alert: the window's alert says it, and Orca presents an alert's line
        // again as the body scrolls it into view.
        assert_eq!(
            crate::native_accessibility::written::get("github-reply-error")
                .map(|line| line.node.role()),
            Some(Role::Status)
        );
        // Exercise actual asynchronous refresh failures with a moved provider head.
        // A failed single-thread read invalidates only its exact context; a failed
        // page read invalidates every retained thread whose snapshot it checked.
        for whole_page in [false, true] {
            cx.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    panel.review.fixture = true;
                    panel.review.fixture_moved = true;
                    if whole_page {
                        panel.thread_page(None, window, cx);
                    } else {
                        panel.thread_comments(target.clone(), None, window, cx);
                    }
                })
            });
            app.read_with(cx, |app, _| app.operations.submit(|| Ok(())))
                .await
                .unwrap()
                .unwrap();
            cx.executor().run_until_parked();
            cx.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    assert!(
                        panel
                            .error
                            .as_ref()
                            .is_some_and(|error| error.contains("could not be loaded"))
                    );
                    assert_eq!(panel.conversations.active.as_ref(), Some(&target));
                    assert_eq!(
                        panel.conversations.input.read(cx).value(),
                        " Exact reply\r\n  café\n"
                    );
                    let thread = &panel.conversations.threads[0];
                    assert!(
                        !thread.reply_available()
                            && !thread.viewer_can_resolve
                            && !thread.viewer_can_unresolve,
                        "all rendered action buttons use the invalidated availability"
                    );
                    assert!(
                        !thread.comments.is_empty(),
                        "failed refresh retains visible conversation text"
                    );
                    assert_eq!(
                        panel.conversations.invalid_threads.len(),
                        if whole_page {
                            panel.conversations.threads.len()
                        } else {
                            1
                        }
                    );
                    if !whole_page {
                        assert!(
                            panel.conversations.threads[1].viewer_can_reply,
                            "unrelated conversation keeps its validated permissions"
                        );
                    }
                    panel.review_reply(window, cx);
                    assert!(
                        panel.confirm.is_none(),
                        "composer cannot review an unvalidated target"
                    );
                    panel.review_resolution(target.clone(), true, window, cx);
                    assert!(
                        panel.confirm.is_none(),
                        "resolution cannot use stale cached permissions"
                    );
                    panel.review.fixture_moved = false;
                    if whole_page {
                        panel.thread_page(None, window, cx);
                    } else {
                        panel.thread_comments(target.clone(), None, window, cx);
                    }
                })
            });
            app.read_with(cx, |app, _| app.operations.submit(|| Ok(())))
                .await
                .unwrap()
                .unwrap();
            cx.executor().run_until_parked();
            cx.update(|window, cx| panel.update(cx, |panel, cx| {
                assert!(panel.conversations.invalid_threads.is_empty());
                assert!(panel.conversations.threads[0].reply_available());
                assert!(panel.conversations.threads[0].viewer_can_resolve);
                assert_eq!(panel.conversations.input.read(cx).value(), " Exact reply\r\n  café\n");
                panel.review_reply(window, cx);
                assert!(matches!(panel.confirm.as_ref().map(|confirmation|&confirmation.action), Some(Action::Reply {target:captured,..}) if captured == &target));
            }));
        }
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.account = Some("another-account".into());
                panel.confirm = None;
                panel.review_reply(window, cx);
                assert!(panel.confirm.is_none());
                assert!(panel.error.is_some());
                panel.account = Some(target.account.clone());
                let original = Draft::Reply(ReplyDraft {
                    target: target.clone(),
                    body: " Exact reply\r\n  café\n".into(),
                });
                panel.conversations.active = None;
                panel.saved = vec![original.clone()];
                panel.conversations.threads[0].target.pull.head.sha = "9".repeat(40);
                panel.restore_reply(window, cx);
                assert!(
                    panel.conversations.active.is_none(),
                    "old head stays copy-only"
                );
                panel.conversations.threads[0].target = target.clone();
                panel.restore_reply(window, cx);
                assert_eq!(panel.conversations.active.as_ref(), Some(&target));
                panel.finish_conversation_action(
                    &Action::Reply {
                        target: target.clone(),
                        body: " Exact reply\r\n  café\n".into(),
                    },
                    window,
                    cx,
                );
                assert!(panel.conversations.active.is_none());
                assert!(
                    !panel
                        .saved
                        .iter()
                        .any(|draft| draft.key() == original.key()),
                    "successful reply cleanup removes its recovery row and count immediately"
                );
                panel.restore_reply(window, cx);
                assert!(
                    panel.conversations.active.is_none(),
                    "confirmed sent text cannot silently restore"
                );
                panel.close(window, cx);
            })
        });
        app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(())))
            .await
            .unwrap()
            .unwrap();
        cx.executor().run_until_parked();
        let saved = app
            .read_with(cx, |app, _| app.preferences_writer.submit(drafts::load))
            .await
            .unwrap()
            .unwrap();
        assert!(
            !saved
                .iter()
                .any(|d| matches!(d,Draft::Reply(reply) if reply.target==target)),
            "successful exact reply deletion is durable"
        );
        app.read_with(cx, |app, _| app.operations.submit(|| Ok(())))
            .await
            .unwrap()
            .unwrap();
        cx.executor().run_until_parked();
        // Replacing a focused old-head composer must transfer keyboard focus
        // onto the new visible capture, so Escape still uses the real close path.
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
        cx.executor().run_until_parked();
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.begin_reply(target.clone(), window, cx);
                panel.conversations.input.update(cx, |input, cx| {
                    input.set_value("Keep old-head text after explicit reinspection", window, cx);
                    cx.emit(InputEvent::Change);
                });
                assert!(
                    panel
                        .conversations
                        .input
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(window)
                );
                panel.review.fixture_moved = true;
                panel.inspect(github::review::fixture_pull(true), window, cx);
            })
        });
        app.read_with(cx, |app, _| app.operations.submit(|| Ok(())))
            .await
            .unwrap()
            .unwrap();
        cx.executor().run_until_parked();
        cx.update(|window, cx| {
            assert!(window.has_active_dialog(cx));
            let panel = panel.read(cx);
            assert_eq!(panel.pull.as_ref().unwrap().head.sha, "3".repeat(40));
            assert!(
                panel.conversations.active.is_none(),
                "old reply remains copy-only after head replacement"
            );
            assert!(
                panel.review.section_focus.is_focused(window),
                "new visible Overview owns focus after old composer disappears"
            );
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.simulate_keystrokes("escape");
        cx.update(|window, cx| {
            assert!(
                !window.has_active_dialog(cx),
                "Escape must invoke the controlled dialog cancellation path"
            );
            assert!(panel.read(cx).closed);
            assert!(
                cx.default_global::<WarmPanels>()
                    .0
                    .iter()
                    .any(|entry| entry.panel.entity_id() == panel.entity_id()),
                "Escape retains the panel in its warm cache"
            );
            app.update(cx, |app, cx| app.open_github(window, cx));
        });
        cx.executor().run_until_parked();
        cx.update(|window,cx|panel.update(cx,|panel,cx| {
            assert_eq!(panel.pull.as_ref().unwrap().head.sha,"3".repeat(40));
            assert!(panel.saved.iter().any(|draft|matches!(draft,Draft::Reply(reply) if reply.target==target && reply.body=="Keep old-head text after explicit reinspection")));
            panel.close(window,cx);
        }));
        app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(())))
            .await
            .unwrap()
            .unwrap();
        cx.executor().run_until_parked();
    }
}
