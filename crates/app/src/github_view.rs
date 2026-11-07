//! Native GitHub collaboration. Repository/PR actions capture their destination;
//! network work exists only behind an explicit button or submit confirmation.
use crate::github::{
    self, Action, Client, ComparisonTarget, NewPull, PullRequest, PullStatus, Repository,
    ReviewDraft, ReviewEvent,
    conversations::CapturedThread,
    drafts::{self, Draft},
    transport::SecureStore,
};
use crate::native_accessibility::LiveRegion as _;
use crate::*;
use futures::FutureExt;
use gitturtle_core::{HistoryCancellation, OperationControl};
use gpui_kit::{
    component::{WindowExt, dialog::DialogFooter},
    prelude::FluentBuilder,
};
use std::time::Duration;
mod conversations;
#[cfg(test)]
mod rest_bench;
mod review;
use crate::github::review::UiTransport;
use review::Section;

/// [`native_accessibility::text`] at the panel's 12 pt, whose lines a reveal of the body never
/// leaves cut at its edge ([`review::text_lines`]).
fn label(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
) -> native_accessibility::StaticText<Stateful<Div>> {
    let (id, text) = (id.into(), text.into());
    let (selector, lines) = (id.clone(), id.clone());
    native_accessibility::static_text(
        div()
            .on_children_prepainted(move |children, window, cx| {
                review::text_lines(&lines, children, window, cx)
            })
            .id(id)
            .debug_selector(move || selector.to_string())
            .child(text.clone()),
        text,
    )
    .text_size(crate::appearance::ui_text(12.))
}
/// A [`label`] of " · "-separated phrases that keeps each phrase whole: a line
/// breaks after a separator, between two phrases, and inside a phrase only
/// where that phrase alone is wider than the line. Each phrase draws as
/// `{id}-{index}`, the separator ending the phrase before it; on one line
/// they read as the text does, and the accessible name is the whole text.
/// Like a [`label`]'s, its lines are never left cut at the body's edge.
fn phrases(
    id: &'static str,
    text: impl Into<SharedString>,
) -> native_accessibility::StaticText<Stateful<Div>> {
    let text = text.into();
    let parts = crate::phrase_runs(&text);
    let lines = ElementId::from(id);
    native_accessibility::static_text(
        div()
            .on_children_prepainted(move |children, window, cx| {
                review::text_lines(&lines, children, window, cx)
            })
            .id(id)
            .debug_selector(move || id.into()),
        text,
    )
    .flex()
    .flex_wrap()
    .text_size(crate::appearance::ui_text(12.))
    .children(parts.into_iter().enumerate().map(move |(index, phrase)| {
        div()
            .debug_selector(move || format!("{id}-{index}"))
            .child(phrase)
    }))
}
/// A path's line of the panel, its label and the path, built once for the path it names
/// ([`PathText::new`]), so a frame builds none of its text: the panel's frames clone its
/// shared strings, which count references.
struct PathText {
    prefix: &'static str,
    path: SharedString,
    /// `prefix` and then `path`, whole: the line's accessible name and its tooltip.
    text: SharedString,
}
impl PathText {
    fn new(prefix: &'static str, path: &std::path::Path) -> Self {
        #[cfg(test)]
        PATH_TEXT_BUILT.with(|built| built.set(built.get() + 1));
        let path: SharedString = path.display().to_string().into();
        Self {
            prefix,
            text: format!("{prefix}{path}").into(),
            path,
        }
    }
}
#[cfg(test)]
thread_local! {
    /// How many times a [`PathText`] was built, for a test.
    static PATH_TEXT_BUILT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
/// A [`label`] of one line, `line`'s prefix and then its path, the path elided in its middle,
/// keeping its top directory and its last component, where the line is narrower than both
/// ([`PathLine`]). The whole text is its accessible name and its tooltip.
fn path_label(
    id: &'static str,
    line: &PathText,
) -> native_accessibility::StaticText<Stateful<Div>> {
    let lines = ElementId::from(id);
    let size = crate::appearance::ui_text(12.);
    let tooltip = line.text.clone();
    crate::views::note_control_tooltip(&lines, &tooltip);
    native_accessibility::static_text(
        div()
            .on_children_prepainted(move |children, window, cx| {
                review::text_lines(&lines, children, window, cx)
            })
            .id(id)
            .debug_selector(move || id.into())
            .child(PathLine {
                prefix: line.prefix,
                path: line.path.clone(),
                size,
            }),
        line.text.clone(),
    )
    .text_size(size)
    .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
}
/// Where the panel's text starts, in from its edge: a compact Button's label
/// inset, [`crate::LABEL_INSET`] at the interface size. The dialog's title, the
/// fixture banner, the notices, the PR rows and the labels between them start
/// here, where the Buttons at the edge (the pager, Recover drafts, the tabs)
/// start their labels. They once started at four insets: the title at the
/// edge, the banner 6.5 px in, the Buttons' labels 10 px and the PR rows'
/// text 20.75 px, inside their cards' border and padding.
fn text_inset() -> Pixels {
    appearance::ui_size(crate::LABEL_INSET)
}
/// Pads `pane`, a bordered pane of the panel, by the panel's [`text_inset`] less its 1 px
/// border at its sides and 0.75 rem above and below, so its frame stands with the Buttons'
/// frames at the panel's side and its text starts at the panel's text edge at every interface
/// size (DESIGN.md, "One left edge"): the Conversations pane, the reply and inline comment
/// composers, the confirmation and the connection pane. 0.75 rem inside their border, as all
/// but the Conversations pane once were, their text started 0.75 px past the panel's at 13 pt
/// and 0.65 px at 18 pt, so the reply composer under the Conversations pane stood apart from it.
fn bordered_pane<E: Styled>(pane: E) -> E {
    pane.py_3().px(text_inset() - px(1.))
}
/// Moves the body's scroll up by `grew`, the room its top line took more than in the last
/// frame, before the body places its content, so the content stays where it was drawn, and
/// the viewport the body's last reveal recorded down by `took`, how much further down the
/// body's top stands than in the last frame, so the reveal finds the body it recorded and
/// shows no focus anew ([`Panel::pin_top_line`], [`Panel::unpin_top_line`]). The two differ
/// as the line moves between the content and above the body: the scroll moves by the
/// difference of the line's room in the two places, the body's top by all of the height the
/// line stands above it. Moved by the scroll's shift, the recorded viewport was a body of
/// another size, and a reply closing scrolled back to a focused control the person had
/// scrolled away from.
fn shift_body(
    scroll: &ScrollHandle,
    revealed: &std::cell::RefCell<review::FocusObservation>,
    grew: Pixels,
    took: Pixels,
) {
    if grew != Pixels::ZERO {
        let offset = scroll.offset();
        scroll.set_offset(point(offset.x, offset.y - grew));
    }
    if took != Pixels::ZERO {
        let mut revealed = revealed.borrow_mut();
        revealed.viewport.origin.y += took;
        revealed.viewport.size.height -= took;
    }
}
/// An element that holds `child` and records the layout ID it is laid out under in `laid`,
/// so an element prepainted before it can read where this frame's layout puts it
/// ([`Panel::unpin_top_line`]).
struct LaidOut {
    child: AnyElement,
    laid: std::rc::Rc<std::cell::Cell<Option<LayoutId>>>,
}
impl IntoElement for LaidOut {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for LaidOut {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let id = self.child.request_layout(window, cx);
        self.laid.set(Some(id));
        (id, ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.prepaint(window, cx);
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}
/// One line of the panel's text, `prefix` and then `path` elided in its middle where the line
/// is narrower than both, keeping the path's top directory and its last component, as the
/// header elides the repository's path ([`crate::views::location_tail`]): "Local worktree:
/// /tmp/…/compact". It works the path out for the width layout gives the line, as it
/// prepaints, so the line keeps to one line at every width and text size (decided
/// 2026-11-06): wrapped with a 120-character path, the connection pane's line took six lines
/// at 17 pt in a 440 × 490 window, and no stop of the pane's Tab walk then showed the help's
/// first line whole. Its text is `size` in the font and colour the line is drawn in.
struct PathLine {
    prefix: &'static str,
    path: SharedString,
    size: Pixels,
}
/// What a [`PathLine`]'s text follows from: the font, interface scale and rem size the line
/// is laid out in ([`crate::views::LineKey`], as the header's identity keys on), its text
/// size, the window's scale, the line's width and the path, so a frame at rest measures
/// nothing and a line in another font is measured in it.
#[derive(Clone, PartialEq)]
struct PathLineKey {
    line: crate::views::LineKey,
    size: u32,
    scale: u32,
    width: u32,
    path: SharedString,
}
/// A [`PathLine`]'s text size, the window's rem size and its scale, as bits, whose line height
/// is kept ([`PATH_LINE_HEIGHTS`]).
type PathLineHeightKey = (u32, u32, u32);
thread_local! {
    /// What [`PathLine`]s showed lately, for what each followed from, so windows drawing the
    /// line at other widths, scales or fonts in turn each keep theirs
    /// ([`crate::views::Measured`]): a cache of one slot worked the line out again on each
    /// other's frames, every frame.
    static PATH_LINES: std::cell::RefCell<crate::views::Measured<PathLineKey, SharedString>> =
        const { std::cell::RefCell::new(crate::views::Measured::new()) };
    /// The height of a [`PathLine`]'s one line for the text sizes, rem sizes and scales drawn
    /// lately: reading the window's text style for it in every frame allocates its font
    /// features.
    static PATH_LINE_HEIGHTS: std::cell::RefCell<crate::views::Measured<PathLineHeightKey, Pixels>> =
        const { std::cell::RefCell::new(crate::views::Measured::new()) };
    /// How many times a [`PathLine`]'s text was worked out, for a test.
    #[cfg(test)]
    static PATH_LINE_MEASURED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// The text the last [`PathLine`] drawn showed, for a test.
    #[cfg(test)]
    static PATH_LINE_SHOWN: std::cell::RefCell<Option<SharedString>> =
        const { std::cell::RefCell::new(None) };
}
impl IntoElement for PathLine {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for PathLine {
    type RequestLayoutState = ();
    type PrepaintState = Option<AnyElement>;
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        // As tall as one line of a text element in the line's text style, which its parent
        // gives it now, its line height snapped to a device pixel as that element snaps it,
        // and as wide as its parent.
        let at = (
            f32::from(self.size).to_bits(),
            f32::from(window.rem_size()).to_bits(),
            window.scale_factor().to_bits(),
        );
        let height = PATH_LINE_HEIGHTS.with_borrow_mut(|kept| {
            *kept.get(at, |_| {
                let (style, rem) = (window.text_style(), window.rem_size());
                let size = style.font_size.to_pixels(rem);
                window.pixel_snap(style.line_height.to_pixels(size.into(), rem))
            })
        });
        let style = Style {
            size: Size {
                width: relative(1.).into(),
                height: height.into(),
            },
            ..Default::default()
        };
        (window.request_layout(style, [], cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let key = PathLineKey {
            line: crate::views::LineKey::current(window),
            size: f32::from(self.size).to_bits(),
            scale: window.scale_factor().to_bits(),
            width: f32::from(bounds.size.width).to_bits(),
            path: self.path.clone(),
        };
        let shown = PATH_LINES.with_borrow_mut(|kept| {
            kept.get(key, |_| self.work_out(bounds.size.width, window))
                .clone()
        });
        #[cfg(test)]
        PATH_LINE_SHOWN.with_borrow_mut(|kept| *kept = Some(shown.clone()));
        // One line that fits its box whole, or, where not even "…/" and the path's last
        // component elided fit beside the prefix, elided in its middle inside it.
        let mut line = crate::views::ellipsis_middle(div().child(shown)).into_any_element();
        line.layout_as_root(bounds.size.map(AvailableSpace::Definite), window, cx);
        line.prepaint_at(bounds.origin, window, cx);
        Some(line)
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        line: &mut Option<AnyElement>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(line) = line {
            line.paint(window, cx);
        }
    }
}
impl PathLine {
    /// The line's text in `width`: the prefix and the path elided in the room the prefix
    /// leaves ([`crate::views::location_tail`]).
    fn work_out(&self, width: Pixels, window: &Window) -> SharedString {
        #[cfg(test)]
        PATH_LINE_MEASURED.with(|measured| measured.set(measured.get() + 1));
        // Each part measured as layout places it; the line's own middle elision keeps its
        // pixel of slack ([`crate::views::ellipsis_middle`]).
        let measure = |text: &str| {
            crate::views::laid_out_text_width(
                crate::views::shaped_width(text, self.size, FontWeight::NORMAL, window),
                window,
            )
        };
        let room = width - px(1.) - measure(self.prefix);
        let path = crate::views::location_tail(&self.path, room, measure);
        format!("{}{path}", self.prefix).into()
    }
}
/// The dialog's own height around the panel's body in `window`: its 1 px edge above and
/// below, the kit's 16 px padding above the title and below the footer and its 16 px gap
/// above the footer (`gpui_kit::component::dialog`), the title's one line, a rem with a line
/// height of one, and the half rem under it, and the footer's compact Button, 28 px at the
/// default interface size (`crate::button_frame`).
fn dialog_chrome(window: &Window) -> Pixels {
    const EDGE: Pixels = px(1.);
    const PADDING: Pixels = px(16.);
    EDGE * 2. + PADDING * 3. + window.rem_size() * 1.5 + appearance::ui_size(28.)
}
/// The panel's width in `window`: at most 1,100 px, less where the window is narrower
/// ([`appearance::dialog_width`]).
fn panel_width(window: &Window) -> Pixels {
    appearance::dialog_width(px(1100.), window)
}
/// The tallest the panel's scrolling body grows in `window`, its padding included: the room
/// the dialog's height cap leaves it ([`appearance::dialog_max_height`], the bound every
/// dialog takes, from a tenth of the window down to the margin above the status bar), less
/// the dialog's own chrome ([`dialog_chrome`]), so the body takes the height its content
/// needs up to that room and scrolls as one past it, and the dialog stays inside its cap. A
/// window less a fixed 220 px scaled with the interface text once stood for that room: at
/// 18 pt it took the dialog's unscaled padding and a tenth of the window as text, and in a
/// 461 × 490 window left the body 185 px of the 265 px the cap gives it, shorter than the
/// connection pane's help in a wider font. It never falls below 120 px, where the panel
/// would show too little to use.
fn body_max_height(window: &Window) -> Pixels {
    (appearance::dialog_max_height(window) - dialog_chrome(window)).max(px(120.))
}
/// One line of a PR row: left-aligned in its column and cut with an ellipsis
/// where the row is too narrow, the whole text in the row's tooltip.
fn row_text(selector: String, text: String) -> Div {
    div()
        .debug_selector(move || selector.clone())
        .flex_1()
        .min_w_0()
        .whitespace_nowrap()
        .text_ellipsis()
        .child(text)
}
/// Preserve native focused-button Enter/Space activation inside this long-lived
/// dialog; its default Return-to-confirm action must not bypass Panel::close.
/// A labelled one is one line of the body, which a reveal never leaves cut at the
/// body's edge ([`review::one_line`]); an icon-only one keeps its square frame,
/// which a child would turn into a labelled Button's.
fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    symbol: &str,
    active: bool,
) -> Button {
    let label = label.into();
    let labelled = !label.is_empty();
    crate::button(id, label, symbol, active)
        .key_context("GitTurtleGithubButton")
        .when(labelled, |button| button.child(review::one_line()))
}
/// [`copy_feedback::copy_button`] with the same activation as [`button`], one
/// line of the body as a labelled [`button`] is.
fn copy_button(
    mark: copy_feedback::Mark,
    label: impl Into<SharedString>,
    tooltip: impl Into<SharedString>,
    cx: &App,
) -> Button {
    let label = label.into();
    let labelled = !label.is_empty();
    copy_feedback::copy_button(mark, label, tooltip, cx)
        .key_context("GitTurtleGithubButton")
        .when(labelled, |button| button.child(review::one_line()))
}

#[derive(Default)]
struct WarmPanels(Vec<WarmPanel>);
impl gpui_kit::Global for WarmPanels {}
struct WarmPanel {
    owner: gpui_kit::EntityId,
    worktree: std::path::PathBuf,
    panel: Entity<Panel>,
    bytes: usize,
}
impl WarmPanels {
    fn retain(&mut self, entry: WarmPanel) {
        self.0
            .retain(|old| old.owner != entry.owner || old.worktree != entry.worktree);
        self.0.push(entry);
        while self.0.len() > 8
            || self.0.iter().map(|entry| entry.bytes).sum::<usize>() > 32 * 1024 * 1024
        {
            self.0.remove(0);
        }
    }
}
impl GitTurtle {
    /// Open the panel from the header's Pull requests button, full or
    /// compact, which closing returns focus to.
    pub(super) fn open_github_from_header(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let opener = self.pull_requests_focus.clone();
        self.open_github_returning_to(Some(opener), window, cx);
    }
    /// Returns once the panel's local read, which [`Self::open_github`]
    /// starts once the dialog's frame is drawn, has landed and been drawn. It
    /// runs on the app's operation executor, a thread `run_until_parked`
    /// cannot wait for: once it drains the read has answered, which holds the
    /// test clock, and the reply lands as the test parks.
    #[cfg(test)]
    pub(crate) fn github_local_read_lands(app: &Entity<Self>, cx: &mut VisualTestContext) {
        crate::views::tests::draw(cx);
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::draw(cx);
    }

    /// Open the panel from the command palette, which has already returned
    /// focus to what it opened over, or from anywhere else: closing returns
    /// focus to what is focused now.
    pub(super) fn open_github(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let previous = window.focused(cx);
        self.open_github_returning_to(previous, window, cx);
    }
    fn open_github_returning_to(
        &mut self,
        opener: Option<FocusHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation_busy.is_some() || self.page != AppPage::Repository {
            return;
        }
        let Some(repo) = self.repository.clone() else {
            return;
        };
        // Closing returns focus here, and so does the repair a frame after
        // the dialog closes. The window's root shows no focus, so with
        // nothing else focused the mode's own list takes it instead.
        self.modal_return_focus = opener.filter(|focus| *focus != self.app_focus);
        review::init(cx);
        let owner = cx.entity().downgrade();
        let branch = self.remote_branch.read(cx).value().to_string();
        let owner_id = cx.entity_id();
        let worktree = repo.path().to_owned();
        let warm = cx.default_global::<WarmPanels>();
        let cached = warm
            .0
            .iter()
            .position(|entry| entry.owner == owner_id && entry.worktree == worktree)
            .map(|index| warm.0.remove(index).panel);
        let form = if let Some(form) = cached {
            form.update(cx, |panel, cx| {
                panel.closed = false;
                panel.confirm = None;
                // Reopening onto its destination field, its first part, the panel shows its
                // body from the top, the banner above the field included. Closed with the body
                // scrolled down, it reopened there, the field at the body's edge and what lies
                // above it out of view, since the field alone was whole. An open PR or a new
                // one keeps the body where it was, beside what reopening focuses.
                if panel.pull.is_none() && !panel.create {
                    panel.review.body_scroll.set_offset(point(px(0.), px(0.)));
                }
                cx.notify();
            });
            form
        } else {
            cx.new(|cx| Panel::new(owner, repo, branch, window, cx))
        };
        let focus_form = form.clone();
        window.defer(cx, move |window, cx| {
            focus_form.update(cx, |panel, cx| {
                panel.load_local(window, cx);
                // Closing cancelled a pending preparation; resume it in whichever mode the
                // file was left, keeping its scroll and Source editor.
                if let Some(index) = panel.review.take_suspended() {
                    panel.prepare_review_file(index, window, cx);
                }
                panel.focus_visible_section(window, cx);
            });
        });
        window.open_alert_dialog(cx, move |dialog, window, _| {
            let close = form.clone();
            let cancel = form.clone();
            dialog
                // In the dialog title's own style, at the panel's text inset.
                .title(
                    native_accessibility::dialog_title("github-title", "GitHub pull requests")
                        .ml(text_inset()),
                )
                .on_ok(|_, _, _| false)
                .width(panel_width(window))
                // No `appearance::dialog_max_height` here: the panel bounds
                // its own scrolling body to the room that cap leaves it
                // (`body_max_height`), and reveals the control Tab moves onto
                // there; the dialog's cap would clip that body and scroll it
                // a second time.
                .child(form.clone())
                .footer(
                    DialogFooter::new().child(
                        button("github-close", "Back to repository", "", false)
                            .debug_selector(|| "github-close".into())
                            .on_click(move |_, window, cx| {
                                close.update(cx, |this, cx| this.close(window, cx))
                            }),
                    ),
                )
                .on_cancel(move |_, window, cx| {
                    cancel.update(cx, |this, cx| this.close(window, cx));
                    false
                })
        });
    }

    /// Submits a request of the panel over `repository`, of `kind`, on the
    /// operation executor, where it runs as the window's busy spell until it
    /// answers, as a write does, labelled with the kind's busy line: Git
    /// writes and preparations refuse meanwhile and say they wait for the
    /// GitHub request ([`Self::busy_reason`]), its Cancel is the operation
    /// bar's, and a working-state read asked for meanwhile waits for it,
    /// then runs as it ends ([`Self::read_deferred_working`]). That holds
    /// after the panel has closed and dropped its reply, as closing during a
    /// read does, since the executor runs the request until it stops. Such a
    /// read once queued behind a review being sent, with nothing saying why,
    /// for as long as GitHub took to answer. Returns the request's control
    /// and its reply, or, submitting nothing while another write,
    /// preparation or request runs, why it waits.
    fn submit_github_request<T: Send + 'static>(
        &mut self,
        kind: RequestKind,
        repository: std::path::PathBuf,
        operation: impl FnOnce(OperationControl) -> anyhow::Result<T> + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<
        (
            OperationControl,
            futures::channel::oneshot::Receiver<anyhow::Result<T>>,
        ),
        &'static str,
    > {
        if self.operation_busy.is_some() {
            return Err(self.busy_reason());
        }
        let label = kind.label();
        let control = self.begin_operation_control(window, cx);
        let worker_control = control.clone();
        let response = self.operations.submit(move || operation(worker_control));
        self.operation_busy = Some(label);
        self.operation_repository = Some(repository);
        let (reply, replied) = futures::channel::oneshot::channel();
        cx.spawn_in(window, async move |this, cx| {
            let result = response
                .await
                .unwrap_or_else(|_| Err(anyhow::anyhow!(INTERRUPTED)));
            let _ = this.update_in(cx, |this, window, cx| {
                if this.operation_busy == Some(label) {
                    this.operation_busy = None;
                    this.operation_repository = None;
                    this.finish_operation_control(window, cx);
                }
                this.read_deferred_working(window, cx);
                cx.notify();
            });
            let _ = reply.send(result);
        })
        .detach();
        cx.notify();
        Ok((control, replied))
    }
}
#[derive(Clone)]
struct Confirmation {
    action: Action,
    account: String,
}
/// The lines the panel says through the window's announcements ([`Panel::announce`]): its
/// notice, its failure, drawn in its body or in the reply composer, and refused input, with
/// the presses that answered with them.
#[derive(Default, PartialEq)]
struct Heard {
    notice: Option<String>,
    error: Option<String>,
    cancelled: Option<&'static str>,
    issue: Option<DraftIssue>,
    presses: Presses,
}
/// How many presses the panel has answered with its failure line and with its refused-input
/// line, so that a press answered as the one before it was is said again: the lines alone
/// stay the same, and a repeated refusal was silent.
#[derive(Clone, Copy, Default, PartialEq)]
struct Presses {
    error: usize,
    issue: usize,
}
/// The control a request or refusal of the panel started from, where its failure, or that
/// Cancel stopped it, answers ([`Panel::outcome`]): feedback answers on the control the person
/// touched. The panel records it as the request starts or the press is refused, and restores it
/// as the request answers. While a conversation was open, every outcome once answered in the
/// reply's composer, a comparison's cancel from Overview included.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Origin {
    /// The composer of the reply to this conversation: its Review reply… and the Send of its
    /// reply. Its outcome stands in that composer, under the draft, while that composer is
    /// drawn, and otherwise on the panel's top line, never in another conversation's composer:
    /// a reply refused on one conversation once showed under the draft of the next one that
    /// Reply opened.
    Reply(Box<CapturedThread>),
    /// Every other control (Open captured local comparison, Connect, Disconnect, Resolve and
    /// Reopen, Refresh PRs, the templates): its outcome stands on the panel's top line.
    Panel,
}
/// What a request of the panel does, which its busy line names, as the
/// window's operation bar does while it runs as the window's busy spell
/// ([`GitTurtle::submit_github_request`]), and why the controls it refuses
/// wait ([`Self::reason`]). Connecting the account and Disconnect once read
/// "Loading your requested PR context…" there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RequestKind {
    /// Reads PR context from GitHub: the list, a PR with its checks, files
    /// and conversations, or a submission's preparation.
    Load,
    /// Sends one confirmed outbound action.
    Send,
    /// Connects the GitHub CLI's signed-in account.
    Connect,
    /// Removes GitTurtle's stored authorization.
    Disconnect,
    /// Compares a PR's captured base and head in the local repository, for
    /// its Changes.
    Compare,
    /// Reads a new PR's templates from its base revision in the local
    /// repository.
    Templates,
}
/// `$lead`, then what a Connect stopped part-way may have left, from whether the platform's
/// store holds GitTurtle's authorization ([`SecureStore::HOLDS`]): on macOS and Linux it may
/// already be stored, which Disconnect removes; elsewhere nothing is stored.
macro_rules! after_stopped_connect {
    ($lead:literal) => {
        if SecureStore::HOLDS {
            concat!(
                $lead,
                " Authorization may already be stored; Disconnect removes it."
            )
        } else {
            concat!(
                $lead,
                " GitTurtle stores no authorization on this platform."
            )
        }
    };
}
/// The connection pane's help: what Connect does, from whether the platform's store holds
/// GitTurtle's authorization ([`SecureStore::HOLDS`]), as a stopped Connect is worded
/// (`after_stopped_connect`): on macOS and Linux it saves the authorization in the store
/// [`SecureStore::NAME`] names (macOS Keychain, or your desktop keyring); elsewhere the store
/// saves none, so Connect cannot finish there.
fn connection_help() -> String {
    let connect = if SecureStore::HOLDS {
        format!(
            "Connect uses your current GitHub CLI account and saves GitTurtle authorization in {}.",
            SecureStore::NAME
        )
    } else {
        "Connect uses your current GitHub CLI account, but GitTurtle stores no authorization on \
         this platform, so Connect cannot finish here yet."
            .to_owned()
    };
    format!("{connect} Refresh and PR selection are explicit network reads.")
}
impl RequestKind {
    pub(crate) const ALL: [Self; 6] = [
        Self::Load,
        Self::Send,
        Self::Connect,
        Self::Disconnect,
        Self::Compare,
        Self::Templates,
    ];
    /// Its busy line.
    fn label(self) -> &'static str {
        match self {
            Self::Load => "Loading your requested PR context…",
            Self::Send => "Sending the captured review…",
            Self::Connect => "Connecting your GitHub account…",
            Self::Disconnect => "Disconnecting GitTurtle from GitHub…",
            Self::Compare => "Comparing the PR's changes locally…",
            Self::Templates => "Reading the PR templates locally…",
        }
    }
    /// Why a control the window's busy spell refuses waits while this runs
    /// as the spell ([`GitTurtle::busy_reason`]): a request to GitHub names
    /// GitHub, and the panel's local work names itself. One reason, "Wait
    /// for the GitHub request to finish", once stood for every kind, though
    /// Disconnect, a PR's Changes and its templates send nothing to GitHub.
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::Load | Self::Send | Self::Connect => "Wait for the GitHub request to finish",
            Self::Disconnect => "Wait for GitTurtle to disconnect from GitHub",
            Self::Compare => "Wait for the pull request comparison to finish",
            Self::Templates => "Wait for the pull request templates to load",
        }
    }
    /// The operation bar's Cancel while this runs as the busy spell
    /// (`GitTurtle::operation_cancel`): its accessible name, which says what
    /// it cancels, and its tooltip, which says what stops and what may
    /// already have happened. It once named "the running Git operation" and
    /// said it stopped Git and its child processes.
    pub(crate) fn cancel(self) -> (&'static str, &'static str) {
        match self {
            Self::Load => (
                "Cancel the running GitHub request",
                "Stop the GitHub request. It only reads, so nothing on GitHub changes.",
            ),
            Self::Send => (
                "Cancel the running GitHub request",
                "Stop the GitHub request. GitHub may already have applied it; check the destination before submitting again.",
            ),
            Self::Connect => (
                "Cancel connecting your GitHub account",
                after_stopped_connect!("Stop connecting."),
            ),
            Self::Disconnect => (
                "Cancel disconnecting GitTurtle from GitHub",
                "Keep GitTurtle's stored authorization, unless its removal has already begun.",
            ),
            Self::Compare => (
                "Cancel the pull request comparison",
                "Stop comparing the PR's changes locally. Nothing changes.",
            ),
            Self::Templates => (
                "Cancel reading the pull request templates",
                "Stop reading the PR templates locally. Nothing changes.",
            ),
        }
    }
    /// What the panel's own Cancel says it asked for while this runs, as the
    /// operation bar's Cancel says in its tooltip ([`Self::cancel`]): what
    /// stops and what may already have happened. It once said "Your review
    /// draft is retained" while Connect, Disconnect or a comparison ran,
    /// which involve no review draft.
    fn requested(self) -> &'static str {
        match self {
            Self::Load => {
                "Cancellation requested. The request only reads; your drafts are retained."
            }
            Self::Send => {
                "Cancellation requested. Check GitHub before resubmitting; an accepted action is never replayed."
            }
            Self::Connect => after_stopped_connect!("Cancellation requested."),
            Self::Disconnect => {
                "Cancellation requested. GitTurtle keeps its stored authorization unless its removal has already begun."
            }
            Self::Compare => {
                "Cancellation requested. Comparing the PR's changes locally changes nothing."
            }
            Self::Templates => {
                "Cancellation requested. Reading the PR templates locally changes nothing."
            }
        }
    }
    /// What the panel says in its failure's place once Cancel stopped this
    /// request before it changed anything ([`Panel::outcome`]), where the
    /// request can tell: a local read stopped by its cancellation ends with
    /// [`gitturtle_core::ReadCancelled`], and Connect and Disconnect, asked
    /// to stop before they reach the account, with [`NotStarted`]. A
    /// cancelled PR comparison once showed the failure telling the person to
    /// Fetch, and a cancelled Disconnect a failure saying the authorization
    /// was kept. A request to GitHub has none: the GitHub CLI's stop reaches
    /// it as a failure, and a sent action's outcome stays uncertain.
    /// `connected` is whether the panel shows a connected account, which a
    /// stopped Connect leaves as it was: it once said no account was
    /// connected while one was.
    fn cancelled(self, connected: bool) -> Option<&'static str> {
        match self {
            Self::Load | Self::Send => None,
            Self::Connect if connected => Some("Cancelled · the account is unchanged"),
            Self::Connect => Some("Cancelled · no account was connected"),
            Self::Disconnect => Some("Cancelled · GitTurtle's authorization was kept"),
            Self::Compare => Some("Cancelled · the PR's changes were not compared"),
            Self::Templates => Some("Cancelled · the PR templates were not read"),
        }
    }
    /// Whether it sends an outbound action, whose outcome may be uncertain
    /// once cancelled: the panel stays open while it runs.
    fn writing(self) -> bool {
        self == Self::Send
    }
}
/// The request of the panel that `label`, the window's busy spell, runs, if
/// it is one.
pub(crate) fn running_request(label: &str) -> Option<RequestKind> {
    RequestKind::ALL
        .into_iter()
        .find(|kind| kind.label() == label)
}
/// Each request's busy line, the window's busy label while it runs.
#[cfg(test)]
pub(crate) fn request_labels() -> [&'static str; RequestKind::ALL.len()] {
    RequestKind::ALL.map(RequestKind::label)
}
/// Whether a request's failure is its cancellation, which stopped it before
/// it changed anything, rather than a failure it met on its own.
fn stopped_by_cancel(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<gitturtle_core::ReadCancelled>()
        .is_some()
        || error.downcast_ref::<NotStarted>().is_some()
}
/// The error a request ends with when Cancel came before it began, so it
/// reached nothing: Connect read no token and Disconnect removed nothing.
#[derive(Debug)]
struct NotStarted;
impl std::fmt::Display for NotStarted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Cancelled before it began")
    }
}
impl std::error::Error for NotStarted {}
/// Ends a request with [`NotStarted`] where Cancel came before it began.
fn not_cancelled(control: &OperationControl) -> anyhow::Result<()> {
    if control.is_cancelled() {
        return Err(NotStarted.into());
    }
    Ok(())
}
/// Where Connect and Disconnect reach the account: the GitHub CLI's signed-in account and
/// the platform's secure store ([`github::transport::SecureStore`]).
#[cfg(not(test))]
mod account {
    use crate::github::transport::{CredentialStore, GhTransport, SecureStore};
    use gitturtle_core::OperationControl;
    pub(super) fn connect(control: &OperationControl) -> anyhow::Result<String> {
        GhTransport::connect_existing(control)
    }
    /// `control` is the request's own, so its Cancel stops a keyring prompt on Linux.
    pub(super) fn remove(control: &OperationControl) -> anyhow::Result<()> {
        SecureStore.remove(control)
    }
}
/// A test build's stand-in for the account, which answers as a signed-in GitHub CLI and an
/// emptied store would, so no test starts the CLI, reads a developer's token or removes their
/// stored authorization (a Keychain item on macOS, a desktop keyring item on Linux), even
/// where a cancellation guard regressed: a Disconnect test once reached the real store's
/// removal with nothing but the guard in its way.
#[cfg(test)]
mod account {
    use gitturtle_core::OperationControl;
    use std::sync::Mutex;
    /// The account the stand-in connects.
    pub(super) const LOGIN: &str = "stand-in-reviewer";
    /// Every control a removal got, so a test can tell it was the request's own, whose
    /// Cancel stops the keyring's prompt, each with the settings directory of the test whose
    /// operation executor ran it, which an executor inherits from the test that started it
    /// ([`crate::preferences::test_settings_directory`]), so a test reads its own removals
    /// only ([`removed_with`]). Every test in the process shares the record: read whole, a
    /// test running beside another that disconnects could take the other's control for its
    /// own.
    static REMOVED_WITH: Mutex<Vec<(std::path::PathBuf, OperationControl)>> =
        Mutex::new(Vec::new());
    pub(super) fn connect(_: &OperationControl) -> anyhow::Result<String> {
        Ok(LOGIN.into())
    }
    pub(super) fn remove(control: &OperationControl) -> anyhow::Result<()> {
        let test = crate::preferences::test_settings_directory();
        REMOVED_WITH
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push((test.path().to_owned(), control.clone()));
        Ok(())
    }
    /// The controls the removals this test's executors ran got ([`REMOVED_WITH`]).
    pub(super) fn removed_with() -> Vec<OperationControl> {
        let test = crate::preferences::test_settings_directory();
        REMOVED_WITH
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .filter(|(directory, _)| directory == test.path())
            .map(|(_, control)| control.clone())
            .collect()
    }
}
/// What a request's reply says when none arrives.
const INTERRUPTED: &str = "GitHub operation was interrupted. Its outcome may be uncertain; check the destination before submitting again.";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DraftIssue {
    Destination,
    Selection,
}
impl DraftIssue {
    fn message(self) -> &'static str {
        match self {
            Self::Destination => {
                "Enter the destination as owner/repository or a GitHub.com remote URL."
            }
            Self::Selection => "Choose a pull request or select New pull request first.",
        }
    }
    fn save_message(self) -> &'static str {
        match self {
            Self::Destination => {
                "Draft not saved: enter a valid owner/repository destination. Keep this window open or copy your text."
            }
            Self::Selection => {
                "Draft not saved: choose a pull request or select New pull request. Keep this window open or copy your text."
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct DraftFields {
    title: String,
    body: String,
    head: String,
    base: String,
}
fn capture_draft(
    destination: &str,
    creating: bool,
    draft: bool,
    event: ReviewEvent,
    pull: Option<&PullRequest>,
    fields: DraftFields,
) -> Result<Draft, DraftIssue> {
    if !creating && pull.is_none() {
        return Err(DraftIssue::Selection);
    }
    let repository = Repository::parse(destination).map_err(|_| DraftIssue::Destination)?;
    if creating {
        Ok(Draft::Pull(NewPull {
            repository,
            title: fields.title,
            body: fields.body,
            head: fields.head,
            base: fields.base,
            draft,
            expected_head: None,
            expected_base: None,
        }))
    } else {
        Ok(Draft::Review(ReviewDraft {
            pull: pull.expect("selection checked above").capture(repository),
            body: fields.body,
            event,
            comments: vec![],
            composing: None,
            discussion: false,
        }))
    }
}
fn empty_pull_draft(draft: &Draft) -> bool {
    matches!(draft, Draft::Pull(pull) if pull.title.is_empty() && pull.body.is_empty())
}
/// One unpublished form snapshot, regardless of how often typing changes its
/// destination key. Only a quiet form or an explicit transition reaches the
/// durable saver; previously accepted destination drafts remain independent.
#[derive(Default)]
struct DeferredDraft {
    generation: u64,
    draft: Option<Draft>,
}
impl DeferredDraft {
    fn clear(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.draft = None;
    }
    fn replace(&mut self, draft: Draft) -> u64 {
        self.clear();
        self.draft = Some(draft);
        self.generation
    }
    fn take(&mut self, generation: u64) -> Option<Draft> {
        if self.generation == generation {
            self.draft.take()
        } else {
            None
        }
    }
    fn is_pending(&self) -> bool {
        self.draft.is_some()
    }
}
const DRAFT_QUIET_PERIOD: Duration = Duration::from_millis(500);
/// The GitHub CLI sign-in command that Copy sign-in command copies.
const SIGN_IN_COMMAND: &str = "gh auth login --hostname github.com --web --skip-ssh-key";
type DraftSaveReply = futures::channel::oneshot::Receiver<anyhow::Result<()>>;
type DraftSaveCompletion = futures::future::Shared<futures::future::BoxFuture<'static, bool>>;

fn track_save_completion(
    latest: &mut Option<DraftSaveCompletion>,
    response: Option<DraftSaveReply>,
) -> Option<DraftSaveCompletion> {
    response.map(|response| {
        let completion = response
            .map(|result| matches!(result, Ok(Ok(()))))
            .boxed()
            .shared();
        *latest = Some(completion.clone());
        completion
    })
}

fn retain_window_close_completion(response: DraftSaveCompletion, cx: &mut App) {
    let mut response = Some(response);
    // The panel and its subscriptions disappear before macOS's asynchronous
    // terminate callback. Keep this accepted completion at app lifetime instead.
    cx.on_app_quit(move |_| {
        let response = response.take();
        async move {
            if let Some(response) = response {
                let _ = response.await;
            }
        }
    })
    .detach();
}

fn apply_save_completion(
    status: &mut SharedString,
    response_generation: u64,
    latest_generation: u64,
    current: Result<Draft, DraftIssue>,
    pending: bool,
    succeeded: bool,
) {
    if response_generation == latest_generation {
        *status = finished_save_status(current, pending, succeeded).into();
    }
}

fn finished_save_status(
    current: Result<Draft, DraftIssue>,
    pending: bool,
    succeeded: bool,
) -> &'static str {
    match current {
        Err(issue) => issue.save_message(),
        Ok(draft) if empty_pull_draft(&draft) => "",
        Ok(_) if !succeeded => "Draft save failed — keep this window open or copy the text",
        Ok(_) if pending => "Saving local draft…",
        Ok(_) => "Draft saved locally",
    }
}
struct Panel {
    review: review::ReviewState,
    conversations: conversations::State,
    owner: WeakEntity<GitTurtle>,
    repo: GitRepository,
    /// The connection pane's "Local worktree:" line for `repo`, built with the panel.
    worktree: PathText,
    destination: Entity<InputState>,
    head: Entity<InputState>,
    base: Entity<InputState>,
    title: Entity<InputState>,
    body: Entity<TextareaState>,
    rows: Vec<PullRequest>,
    page: u32,
    has_next: bool,
    pull: Option<PullRequest>,
    /// The PR row a left press is held down on, which draws the whole row pressed, title and
    /// context line alike. The row records it as the press is captured, since the title
    /// Button keeps the press from GPUI's own pressed state for anything around it; any
    /// release ends it.
    pressed: std::rc::Rc<std::cell::Cell<Option<u64>>>,
    status: Option<PullStatus>,
    description: Option<Entity<EditorState>>,
    create: bool,
    draft: bool,
    event: ReviewEvent,
    discussion: bool,
    account: Option<String>,
    notice: Option<String>,
    /// What the panel last said through the window's announcements ([`Panel::announce`]),
    /// cleared as it closes, so that it says again what it reopens with.
    heard: Heard,
    /// What the top line said in the last frame: when that changes, its text scrolls back to
    /// its first sentence ([`Panel::pin_top_line`]).
    top_line_said: Heard,
    error: Option<String>,
    /// That Cancel stopped the last request ([`RequestKind::cancelled`]),
    /// said in `error`'s place until the next request.
    cancelled: Option<&'static str>,
    /// Where `error` or `cancelled` answers: the control the request or refusal they answer
    /// started from ([`Origin`]).
    origin: Origin,
    /// The presses answered with `error` and `draft_issue` ([`Presses`]).
    presses: Presses,
    pending: bool,
    /// The busy row's Cancel, which hands focus to the shown section as the
    /// row goes.
    cancel_request: focus_ring::LoadingCancel,
    request_generation: u64,
    local_loaded: bool,
    local_loading: bool,
    local_generation: u64,
    local_task: Option<Task<()>>,
    /// What the request the panel sent last does, which its busy line names
    /// while it runs.
    request: RequestKind,
    closed: bool,
    confirm: Option<Confirmation>,
    control: Option<OperationControl>,
    task: Option<Task<()>>,
    saved: Vec<Draft>,
    /// The draft's save status, shared with each frame's labels, which copy nothing of it.
    save_status: SharedString,
    draft_issue: Option<DraftIssue>,
    saver: commit_drafts::CoalescingSaver<String, Draft>,
    deferred_draft: DeferredDraft,
    draft_timer: Option<Task<()>>,
    save_response_generation: u64,
    save_completion: Option<DraftSaveCompletion>,
    subscriptions: Vec<Subscription>,
    templates: Vec<github::Template>,
    attempts: Vec<drafts::Attempt>,
    local_control: Option<HistoryCancellation>,
}
impl Panel {
    fn new(
        owner: WeakEntity<GitTurtle>,
        repo: GitRepository,
        branch: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let worktree = PathText::new("Local worktree: ", repo.path());
        let mut this=Self {
            review:review::ReviewState::new(window,cx),
            conversations:conversations::State::new(window,cx),
            owner,worktree,repo,destination:cx.new(|cx|InputState::new(window,cx).placeholder("owner/repository")),
            head:cx.new(|cx|InputState::new(window,cx).default_value(branch).placeholder("Published head branch (owner:branch for a fork)")),
            base:cx.new(|cx|InputState::new(window,cx).default_value("main")),title:cx.new(|cx|InputState::new(window,cx).placeholder("Pull request title")),
            body:cx.new(|cx|TextareaState::new(window,cx).auto_grow(5,5).placeholder("Description or review text — saved locally")),
            rows:vec![],page:1,has_next:false,pull:None,pressed:Default::default(),status:None,description:None,create:false,draft:true,event:ReviewEvent::Comment,discussion:true,
            account:None,notice:Some("Offline until you choose Connect or Refresh. GitHub.com is supported; Git authentication and Push keep their existing behavior.".into()),heard:Heard::default(),top_line_said:Heard::default(),error:None,cancelled:None,origin:Origin::Panel,presses:Presses::default(),pending:false,cancel_request:focus_ring::LoadingCancel::new(cx),request_generation:0,local_loaded:false,local_loading:false,local_generation:0,local_task:None,request:RequestKind::Load,closed:false,
            confirm:None,control:None,task:None,saved:vec![],save_status:SharedString::default(),draft_issue:None,saver:Default::default(),deferred_draft:Default::default(),draft_timer:None,save_response_generation:0,save_completion:None,subscriptions:vec![],templates:vec![],attempts:vec![],local_control:None,
        };
        if this.review.fixture {
            this.destination.update(cx, |input, cx| {
                input.set_value("gitturtle-fixture/native-review", window, cx)
            });
            this.account = Some("offline-reviewer".into());
            this.notice=Some("Offline review fixture · fixed sample data; account connection and network transport are unavailable in this mode.".into());
        }
        this.subscriptions.push(cx.subscribe_in(
            &this.review.input,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.confirm = None;
                    this.defer_draft_save(window, cx);
                }
            },
        ));
        this.subscriptions.push(cx.subscribe_in(
            &this.conversations.input,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.confirm = None;
                    this.defer_reply_save(window, cx);
                }
            },
        ));
        for input in [this.head.clone(), this.base.clone(), this.title.clone()] {
            this.subscriptions.push(cx.subscribe_in(
                &input,
                window,
                |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.confirm = None;
                        this.defer_draft_save(window, cx);
                    }
                },
            ));
        }
        this.subscriptions.push(cx.subscribe_in(
            &this.body,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.confirm = None;
                    this.defer_draft_save(window, cx);
                }
            },
        ));
        this.subscriptions.push(cx.subscribe_in(
            &this.destination,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    // Destination edits arrive after the field changed. Flush the
                    // old captured review snapshot before clearing its identity.
                    if let Some(draft @ Draft::Review(_)) = this.deferred_draft.draft.take() {
                        this.draft_timer = None;
                        this.persist_draft(draft, window, cx);
                    }
                    this.save_reply(window, cx);
                    this.conversations.reset_content();
                    this.review.reset_content();
                    this.review.comments.clear();
                    this.review.composing = None;
                    this.review.show_list = true;
                    this.rows.clear();
                    this.pull = None;
                    this.status = None;
                    this.description = None;
                    this.confirm = None;
                    this.page = 1;
                    this.has_next = false;
                    this.defer_draft_save(window, cx);
                    cx.notify();
                }
            },
        ));
        let panel = cx.weak_entity();
        let panel_window = window.window_handle().window_id();
        this.subscriptions.push(cx.on_window_closed(move |cx, id| {
            if id == panel_window {
                // GPUI delivers this before releasing the window's entities.
                if let Ok(Some(response)) =
                    panel.update(cx, |this, cx| this.flush_draft_for_shutdown(cx))
                {
                    retain_window_close_completion(response, cx);
                }
            }
        }));
        this.subscriptions.push(cx.on_app_quit(|this, cx| {
            let response = this.flush_draft_for_shutdown(cx);
            async move {
                if let Some(response) = response {
                    let _ = response.await;
                }
            }
        }));
        // The parent opens this entity while it is already being updated.
        // Wait until GPUI returns that parent to the app before borrowing its
        // serial executor for passive local metadata and draft reads.
        cx.defer_in(window, |this, window, cx| {
            if !this.closed {
                this.load_local(window, cx);
            }
        });
        this
    }
    fn load_local(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed || self.local_loaded || self.local_loading {
            return;
        }
        self.local_loading = true;
        self.local_generation = self.local_generation.wrapping_add(1);
        let generation = self.local_generation;
        let repo = self.repo.clone();
        let response = self.owner.update(cx, |owner, _| {
            owner.operations.submit_read(move || {
                Ok((repo.remote_configs()?, drafts::load(), drafts::attempts()))
            })
        });
        let Ok(response) = response else {
            self.local_loading = false;
            return;
        };
        self.local_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.closed || generation != this.local_generation {
                    return;
                }
                this.local_loading = false;
                match result {
                    Ok(Ok((remotes, saved, attempts))) => {
                        this.local_loaded = true;
                        if this.destination.read(cx).value().is_empty()
                            && let Some(destination) = remotes
                                .iter()
                                .flat_map(|remote| remote.urls.iter())
                                .find_map(|url| Repository::parse(url).ok())
                        {
                            this.destination.update(cx, |input, cx| {
                                input.set_value(destination.label(), window, cx)
                            });
                        }
                        match attempts {
                            Ok(attempts) => this.attempts = attempts,
                            Err(error) => this.local_read_failed(format!("{error:#}")),
                        }
                        match saved {
                            Ok(mut saved) => {
                                for draft in std::mem::take(&mut this.saved) {
                                    let key = draft.key();
                                    saved.retain(|old| old.key() != key);
                                    saved.push(draft);
                                }
                                this.saved = saved;
                            }
                            Err(error) => this.local_read_failed(format!("{error:#}")),
                        }
                    }
                    _ => this
                        .local_read_failed("Could not read repository remotes and saved drafts."),
                }
                cx.notify();
            });
        }));
    }
    fn destination(&self, cx: &App) -> anyhow::Result<Repository> {
        Repository::parse(&self.destination.read(cx).value())
    }
    fn current_draft(&self, cx: &App) -> Result<Draft, DraftIssue> {
        let mut draft = capture_draft(
            &self.destination.read(cx).value(),
            self.create,
            self.draft,
            self.event,
            self.pull.as_ref(),
            DraftFields {
                title: self.title.read(cx).value().to_string(),
                body: self.body.read(cx).value().to_string(),
                head: self.head.read(cx).value().to_string(),
                base: self.base.read(cx).value().to_string(),
            },
        )?;
        if let Draft::Review(review) = &mut draft {
            review.discussion = self.discussion;
            review.comments = self.review.comments.clone();
            review.composing = self.review.composing.clone().map(|mut comment| {
                comment.body = self.review.input.read(cx).value().to_string();
                comment
            });
        }
        Ok(draft)
    }

    fn draft_to_save(&mut self, cx: &App) -> Option<Draft> {
        let draft = match self.current_draft(cx) {
            Ok(draft) => draft,
            Err(issue) => {
                self.save_status = issue.save_message().into();
                if self.draft_issue.is_some() {
                    self.draft_issue = Some(issue);
                }
                return None;
            }
        };
        self.draft_issue = None;
        if empty_pull_draft(&draft) {
            self.save_status = SharedString::default();
            return None;
        }
        Some(draft)
    }
    fn defer_draft_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.draft_timer = None;
        self.deferred_draft.clear();
        if let Some(draft) = self.draft_to_save(cx) {
            let generation = self.deferred_draft.replace(draft);
            self.save_status = "Saving local draft…".into();
            let timer = cx.background_executor().timer(DRAFT_QUIET_PERIOD);
            self.draft_timer = Some(cx.spawn_in(window, async move |this, cx| {
                timer.await;
                let _ = this.update_in(cx, |this, window, cx| {
                    let Some(draft) = this.deferred_draft.take(generation) else {
                        return;
                    };
                    this.draft_timer = None;
                    if !this.closed {
                        this.persist_draft(draft, window, cx);
                    }
                });
            }));
        }
        cx.notify();
    }
    fn save_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_reply(window, cx);
        // Explicit close/selection/review commits the current form immediately.
        // A superseded timer must never enqueue its older destination afterward.
        self.draft_timer = None;
        self.deferred_draft.clear();
        if let Some(draft) = self.draft_to_save(cx) {
            self.persist_draft(draft, window, cx);
        }
        cx.notify();
    }
    fn flush_draft_for_shutdown(&mut self, cx: &mut App) -> Option<DraftSaveCompletion> {
        self.draft_timer = None;
        self.deferred_draft.clear();
        let reply = if self.closed {
            None
        } else {
            self.reply_snapshot(cx)
        };
        self.conversations.timer = None;
        self.conversations.deferred.clear();
        let draft = if self.closed {
            None
        } else {
            self.draft_to_save(cx)
        };
        self.closed = true;
        if let Ok(response) = self.owner.update(cx, |owner, _| {
            let mut response = None;
            for draft in draft.into_iter().chain(reply) {
                let queued = self.saver.queue_with(
                    &owner.preferences_writer,
                    draft.key(),
                    draft,
                    drafts::save_batch,
                );
                if queued.is_some() {
                    response = queued;
                }
            }
            response
        }) {
            let _ = track_save_completion(&mut self.save_completion, response);
        }
        // Await the actual accepted save, including one this edit coalesced
        // into. A separate barrier could be refused by the bounded executor.
        self.save_completion.clone()
    }
    fn persist_draft(&mut self, draft: Draft, window: &mut Window, cx: &mut Context<Self>) {
        let key = draft.key();
        if matches!(&draft, Draft::Reply(reply) if reply.body.is_empty()) {
            // Empty replies are serialized deletions, not recoverable snapshots.
            // Queue the deletion below while removing it from the visible cache.
            self.saved.retain(|saved| saved.key() != key);
        } else if let Some(old) = self.saved.iter_mut().find(|d| d.key() == key) {
            *old = draft.clone();
        } else {
            self.saved.push(draft.clone());
        }
        let response = match self.owner.update(cx, |owner, _| {
            self.saver
                .queue_with(&owner.preferences_writer, key, draft, drafts::save_batch)
        }) {
            Ok(response) => response,
            Err(_) => {
                self.save_status =
                    "Draft save failed — keep this window open or copy the text".into();
                cx.notify();
                return;
            }
        };
        self.save_status = "Saving local draft…".into();
        if let Some(response) = track_save_completion(&mut self.save_completion, response) {
            self.save_response_generation = self.save_response_generation.wrapping_add(1);
            let response_generation = self.save_response_generation;
            cx.spawn_in(window, async move |this, cx| {
                let succeeded = response.await;
                let _ = this.update_in(cx, |this, _, cx| {
                    // A previous destination's reply must not claim the
                    // currently invalid or newly edited form has been saved.
                    let current = this.current_draft(cx);
                    apply_save_completion(
                        &mut this.save_status,
                        response_generation,
                        this.save_response_generation,
                        current,
                        this.deferred_draft.is_pending()
                            || this.conversations.deferred.is_pending()
                            || this.saver.is_pending(),
                        succeeded,
                    );
                    cx.notify();
                });
            })
            .detach();
        }
        cx.notify();
    }
    /// Answers a press with `error` on the failure line, said at once however often the same
    /// refusal repeats ([`Presses`]): on the panel's top line ([`Origin::Panel`]).
    fn refuse(&mut self, error: impl Into<String>) {
        self.refuse_at(Origin::Panel, error);
    }
    /// Says that local work no request carried failed, on the panel's top line: the panel's
    /// own read as it opens, which no control asked for, or a Files row's preparation, which
    /// runs beside the requests. A preparation's failure once kept the last request's origin
    /// and stood in an open reply's composer after a refused Review reply….
    fn local_read_failed(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
        self.origin = Origin::Panel;
    }
    /// [`Self::refuse`] for a press on the control `origin`, where the failure line answers.
    fn refuse_at(&mut self, origin: Origin, error: impl Into<String>) {
        self.error = Some(error.into());
        self.origin = origin;
        self.presses.error = self.presses.error.wrapping_add(1);
    }
    /// Whether the failure line answers in the open reply's composer, rather than on the
    /// panel's top line: only for what was asked from that composer, the composer of the
    /// conversation it was asked for ([`Origin`]).
    fn answers_in_reply(&self) -> bool {
        matches!(
            (&self.origin, &self.conversations.active),
            (Origin::Reply(asked), Some(open)) if **asked == *open
        )
    }
    /// Answers a press with `issue` on the refused-input line, said at once however often it
    /// repeats ([`Presses`]).
    fn refuse_input(&mut self, issue: DraftIssue) {
        self.draft_issue = Some(issue);
        self.presses.issue = self.presses.issue.wrapping_add(1);
    }
    /// Sends `operation`, a request of `kind`, as the window's busy spell
    /// ([`GitTurtle::submit_github_request`]) and hands its reply to
    /// `receive`. Returns whether it was sent. Nothing is, and nothing more
    /// is said, while the panel's own request runs, which its busy line
    /// names and whose controls are disabled meanwhile, so no press reaches
    /// here then, or once the panel has closed, with no line to say it in.
    /// Nothing is sent either while the window's busy spell runs a Git
    /// write, a preparation or a request the panel left running as it
    /// closed, and then the panel's failure line says why, in the spell's
    /// own reason ([`GitTurtle::busy_reason`]). Its outcome answers on the panel's top line
    /// ([`Origin::Panel`]).
    fn run<T: Send + 'static>(
        &mut self,
        kind: RequestKind,
        operation: impl FnOnce(OperationControl) -> anyhow::Result<T> + Send + 'static,
        receive: impl FnOnce(anyhow::Result<T>, &mut Self, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.run_from(Origin::Panel, kind, operation, receive, window, cx)
    }
    /// [`Self::run`] for a request started from the control `origin`, which it records as it
    /// starts, so its failure, or that Cancel stopped it, answers there whatever the panel
    /// said meanwhile.
    fn run_from<T: Send + 'static>(
        &mut self,
        origin: Origin,
        kind: RequestKind,
        operation: impl FnOnce(OperationControl) -> anyhow::Result<T> + Send + 'static,
        receive: impl FnOnce(anyhow::Result<T>, &mut Self, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.pending || self.closed {
            return false;
        }
        let repository = self.repo.path().to_owned();
        let submitted = self.owner.update(cx, |owner, cx| {
            owner.submit_github_request(kind, repository, operation, window, cx)
        });
        let Ok(submitted) = submitted else {
            self.refuse_at(origin, "The repository window is unavailable.");
            return false;
        };
        let (control, response) = match submitted {
            Ok(submitted) => submitted,
            Err(reason) => {
                self.refuse_at(origin, format!("{reason}, then try again."));
                cx.notify();
                return false;
            }
        };
        self.control = Some(control);
        self.request_generation = self.request_generation.wrapping_add(1);
        let generation = self.request_generation;
        self.pending = true;
        self.request = kind;
        self.error = None;
        self.cancelled = None;
        self.origin = origin.clone();
        self.notice = None;
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response
                .await
                .unwrap_or_else(|_| Err(anyhow::anyhow!(INTERRUPTED)));
            let _ = this.update_in(cx, |this, window, cx| {
                if generation != this.request_generation {
                    return;
                }
                this.pending = false;
                this.control = None;
                // The busy row goes: focus on its Cancel moves to the section
                // the panel shows, as when it opens, before the reply moves it
                // anywhere else.
                if !this.closed && this.cancel_request.holds_focus(window) {
                    this.focus_visible_section(window, cx);
                }
                // It answers where it started, though a refusal meanwhile answered elsewhere.
                this.origin = origin;
                if !this.closed {
                    // Cancel stopped it: the panel says so where a failure
                    // would stand, in place of what asked for the cancel. A
                    // failure the request met before the cancel reached it
                    // ends with its own error and is shown as one.
                    match (result, kind.cancelled(this.account.is_some())) {
                        (Err(error), Some(line)) if stopped_by_cancel(&error) => {
                            this.cancelled = Some(line);
                            this.notice = None;
                        }
                        (result, _) => receive(result, this, window, cx),
                    }
                }
                // `run` cleared the failure line, so one there now is this press's answer.
                if this.error.is_some() {
                    this.presses.error = this.presses.error.wrapping_add(1);
                }
                cx.notify();
            });
        }));
        cx.notify();
        true
    }
    fn connect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.review.fixture {
            self.notice=Some("Offline fixture account: offline-reviewer. No credential or network access is available.".into());
            cx.notify();
            return;
        }
        self.run(
            RequestKind::Connect,
            |control| {
                not_cancelled(&control)?;
                account::connect(&control)
            },
            |result, this, _, _| match result {
                Ok(login) => {
                    this.notice = Some(format!(
                        "Connected as {login}; authorization is stored in {}.",
                        SecureStore::NAME
                    ));
                    this.account = Some(login);
                }
                Err(error) => this.error = Some(format!("{error:#}")),
            },
            window,
            cx,
        );
    }
    /// Removes GitTurtle's stored authorization, unless Cancel came before
    /// the removal began: then it keeps the authorization and says so. Its
    /// removal once ignored Cancel.
    fn disconnect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run(
            RequestKind::Disconnect,
            |control| {
                not_cancelled(&control)?;
                account::remove(&control)
            },
            |result, this, _, _| match result {
                Ok(()) => {
                    this.account = None;
                    this.notice = Some("GitTurtle authorization removed. GitHub CLI and ordinary Git authentication retain their settings.".into());
                }
                Err(error) => this.error = Some(error.to_string()),
            },
            window,
            cx,
        );
    }
    fn refresh(&mut self, page: u32, window: &mut Window, cx: &mut Context<Self>) {
        self.save_draft(window, cx);
        let repository = match self.destination(cx) {
            Ok(repo) => repo,
            Err(error) => {
                self.refuse(error.to_string());
                return;
            }
        };
        let fixture = self.review.fixture;
        let moved = self.review.fixture_moved;
        self.run(RequestKind::Load,move|control|{let transport=UiTransport::open(fixture,moved)?;let login=transport.login().to_owned();Ok((Client(transport).list(&repository,page,&control)?,login))},|result,this,_,_|match result {Ok((page,login))=>{this.create=false;this.review.show_list=true;this.confirm=None;this.rows=page.items;this.page=page.page;this.has_next=page.has_next;this.account=Some(login);this.notice=Some("PR list refreshed. Selecting a PR explicitly loads its current review and check status.".into());},Err(error)=>this.error=Some(format!("{error:#}"))},window,cx);
    }
    fn inspect(&mut self, pull: PullRequest, window: &mut Window, cx: &mut Context<Self>) {
        self.save_draft(window, cx);
        let repository = match self.destination(cx) {
            Ok(repo) => repo,
            Err(error) => {
                self.refuse(error.to_string());
                return;
            }
        };
        let fixture = self.review.fixture;
        let moved = self.review.fixture_moved;
        self.run(RequestKind::Load,move|control|{
            let mut client=Client(UiTransport::open(fixture,moved)?);
            let pull=client.pull(&repository,pull.number,&control)?;
            let captured=pull.capture(repository);
            let status=client.status(&captured,&control);
            let files=client.files(&captured,1,&control);
            let comments=client.review_threads(&captured,None,&control);
            Ok((pull,captured,status,files,comments))
        },|result,this,window,cx|match result {
            Ok((pull,captured,status,files,comments))=>{
                this.create=false;this.confirm=None;this.review.reset_content();this.conversations.reset_content();this.review.show_list=false;this.review.section=Section::Overview;
                this.description=pull.body.as_ref().map(|body|text::editor(body,"markdown",None,window,cx));
                let mut errors=vec![];
                match status {Ok(status)=>this.status=Some(status),Err(error)=>{this.status=None;errors.push(format!("Review/check status: {error:#}"));}}
                match files {Ok(page)=>{this.review.files=page.items;this.review.file_page=page.page;this.review.files_next=page.has_next;},Err(error)=>errors.push(format!("Changed files: {error:#}"))}
                match comments {Ok(page)=>{this.receive_threads(page,None);},Err(error)=>errors.push(format!("Discussions: {error:#}"))}
                this.pull=Some(pull);
                let draft=this.saved.iter().find_map(|draft|match draft {Draft::Review(review) if review.pull==captured=>Some(review.clone()),_=>None});
                this.body.update(cx,|input,cx|input.set_value(draft.as_ref().map(|d|d.body.clone()).unwrap_or_default(),window,cx));
                this.event=draft.as_ref().map(|d|d.event).unwrap_or(ReviewEvent::Comment);
                let had_review=draft.is_some();
                this.restore_review(draft,window,cx);
                this.restore_reply(window,cx);
                let had_saved=had_review||this.conversations.active.is_some();
                let older=this.saved.iter().any(|draft| match draft {
                    Draft::Review(review) => review.pull.repository==captured.repository && review.pull.number==captured.number && review.pull!=captured,
                    Draft::Reply(reply) => !reply.body.is_empty() && reply.target.pull.repository==captured.repository && reply.target.pull.number==captured.number && (reply.target.pull!=captured || this.account.as_deref()!=Some(reply.target.account.as_str())),
                    _ => false,
                });
                if older { this.notice=Some("Drafts from another captured head or account remain in Recover drafts. Copy their exact text and original identities; targets are never remapped.".into()); }
                else if !had_saved { this.notice=None; }
                this.error=(!errors.is_empty()).then(||errors.join("\n"));
                this.save_status=if had_saved {"Draft restored locally"}else{"No local draft yet"}.into();
                this.focus_visible_section(window,cx);
                this.review.opened.set(true);
            },Err(error)=>this.error=Some(format!("{error:#}"))
        },window,cx);
    }

    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.create {
            self.save_draft(window, cx);
            return;
        }
        self.save_draft(window, cx);
        self.create = true;
        self.conversations.reset_content();
        self.review.reset_content();
        self.review.comments.clear();
        self.review.composing = None;
        self.pull = None;
        self.status = None;
        self.description = None;
        self.confirm = None;
        self.body
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.notice=Some("Use a branch already published to GitHub. Creating a PR never pushes a branch. Choose Draft or Ready, then review the destination.".into());
        self.save_draft(window, cx);
        cx.notify();
    }
    fn load_templates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let repo = self.repo.clone();
        let revision = self.base.read(cx).value().to_string();
        let cancellation = HistoryCancellation::default();
        self.local_control = Some(cancellation.clone());
        let stop = cancellation.clone();
        let sent = self.run(RequestKind::Templates,move|_|github::local_templates(&repo,&revision,&cancellation),|result,this,_,_|match result {Ok(templates)=>{this.templates=templates;this.notice=Some("Templates came from the locally available base revision. Choose one to append its exact text; repository HTML is never executed.".into());},Err(error)=>this.error=Some(format!("{error:#}"))},window,cx);
        // The read goes through its own cancellation, as a PR's Changes
        // does, which the panel's Cancel and the operation bar's stop; it
        // once read on to the end and showed the templates after Cancel.
        if sent {
            let _ = self
                .owner
                .update(cx, |owner, _| owner.cancel_with_operation(stop));
        }
    }
    fn review_submission(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_draft(window, cx);
        let draft = match self.current_draft(cx) {
            Ok(draft) => draft,
            Err(issue) => {
                self.refuse_input(issue);
                self.save_status = issue.save_message().into();
                cx.notify();
                return;
            }
        };
        self.draft_issue = None;
        if let Draft::Review(review) = &draft {
            if review.composing.is_some() {
                self.refuse(
                    "Add or discard the unfinished inline comment before reviewing submission.",
                );
                cx.notify();
                return;
            }
            if let Err(error) = github::validate_text(
                &review.body,
                self.discussion
                    || review.event == ReviewEvent::RequestChanges
                    || (review.event == ReviewEvent::Comment && review.comments.is_empty()),
            ) {
                self.refuse(error.to_string());
                cx.notify();
                return;
            }
        }
        let fixture = self.review.fixture;
        let moved = self.review.fixture_moved;
        if let Draft::Pull(pull) = draft {
            self.run(
                RequestKind::Load,
                move |control| {
                    let transport = UiTransport::open(fixture, moved)?;
                    let account = transport.login().to_owned();
                    Ok((Client(transport).prepare_create(pull, &control)?, account))
                },
                |result, this, window, cx| match result {
                    Ok((pull, account)) => {
                        this.account = Some(account.clone());
                        this.confirm = Some(Confirmation {
                            action: Action::Create(pull),
                            account,
                        });
                        this.focus_confirmation(window, cx);
                    }
                    Err(error) => this.error = Some(format!("{error:#}")),
                },
                window,
                cx,
            );
            return;
        }
        let Some(account) = self.account.clone() else {
            self.refuse("Connect and refresh the GitHub account before submission.");
            return;
        };
        self.confirm = Some(Confirmation {
            account,
            action: match draft {
                Draft::Pull(_) | Draft::Reply(_) => unreachable!(),
                Draft::Review(p) if self.discussion => Action::Comment {
                    pull: p.pull,
                    body: p.body,
                },
                Draft::Review(p) => Action::Review(p),
            },
        });
        self.focus_confirmation(window, cx);
    }
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The confirmation goes once its action is sent: one a running Git
        // write refuses stays for Send once the write ends. Taken first, it
        // went with the refusal, and the submission had to be reviewed again.
        let Some(confirmation) = self.confirm.clone() else {
            return;
        };
        self.save_draft(window, cx);
        let fixture = self.review.fixture;
        let moved = self.review.fixture_moved;
        let sent_action = confirmation.action.clone();
        // A reply's Send answers in the reply's composer, where the reply was written; every
        // other action's, a conversation's Resolve or Reopen included, on the panel's top line.
        let origin = match &confirmation.action {
            Action::Reply { target, .. } => Origin::Reply(Box::new(target.clone())),
            _ => Origin::Panel,
        };
        let sent = self.run_from(origin, RequestKind::Send, move |control| {
            let transport = UiTransport::open(fixture, moved)?;
            anyhow::ensure!(transport.login() == confirmation.account, "The connected GitHub account changed. Review the destination again before submitting.");
            let destination = format!("{} · account {}", confirmation.action.destination(), confirmation.account);
            drafts::record_attempt(destination.clone(), "Submission started; if no final outcome appears, inspect GitHub before sending again.".into())?;
            let completed_reply = match &confirmation.action {
                Action::Reply { target, body } => Some(drafts::ReplyDraft { target: target.clone(), body: body.clone() }),
                _ => None,
            };
            let result = Client(transport).execute(confirmation.action, &control);
            let outcome = match &result { Ok(notice) => notice.clone(), Err(error) => format!("{error:#}") };
            if let Err(error) = drafts::record_reply_outcome(destination, outcome.chars().take(4000).collect(), completed_reply.as_ref().filter(|_|result.is_ok())) {
                return Err(anyhow::anyhow!("The GitHub action finished but saving its outcome failed: {error}. Check GitHub before resubmitting."));
            }
            result
        }, move |result, this, window, cx| match result {
            Ok(notice) => {
                if matches!(sent_action, Action::Reply {..} | Action::SetThreadResolved {..}) {
                    this.finish_conversation_action(&sent_action, window, cx);
                } else {
                    this.notice = Some(format!("{notice}. The local draft remains available; submitting it again creates another outbound action."));
                    if matches!(sent_action, Action::Create(_)) {
                        this.create = false; this.review.show_list = true;
                        this.notice = Some("Pull request created. Refresh PRs to open it.".into());
                        this.focus_visible_section(window, cx);
                    }
                }
            },
            Err(error) => {
                this.error = Some(format!("{error:#}"));
                if this.conversations.active.is_some() { this.focus_visible_section(window,cx); }
            },
        }, window, cx);
        if sent {
            self.confirm = None;
        }
    }
    /// Says the lines the panel shows through the window's announcements, once each, as the
    /// panel opens with them and whenever they change: its notice politely
    /// (`copy_feedback::announce`), and its failure and refused input at once
    /// (`copy_feedback::alert`), both leaving a control reading Copied as it is. The lines
    /// are statuses, no live regions: an adapter announces a live region only while
    /// AccessKit's filter keeps its node, which it does not once another child lies between
    /// the node and the body's edge, and again each time the body scrolls it back. At 18 pt in
    /// a 461 × 490 window the notice lies below the fold, under the banner whose Move fixture
    /// head changes it, and a failure lands wherever the body has scrolled. Nor are they
    /// alerts: AT-SPI reports a line's `showing` change as the body scrolls it in or out
    /// (vendored adapter), and Orca presents an alert, a notification there, as it becomes
    /// showing, so a failure the window's alert said could be said again from its line. A
    /// press answered as the one before it was is said again, a fresh alert node for each
    /// press, as a refused Toggle Sidebar is: the lines alone stay the same.
    fn announce(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shown = Heard {
            notice: self.notice.clone(),
            error: self.error.clone(),
            cancelled: self.cancelled,
            issue: self.draft_issue,
            presses: self.presses,
        };
        if shown == self.heard {
            return;
        }
        let heard = std::mem::replace(&mut self.heard, shown);
        let shown = &self.heard;
        // A cancel the person asked for is said politely, as the notice is, not as an alert.
        let notice = [
            shown
                .notice
                .clone()
                .filter(|_| shown.notice != heard.notice),
            shown
                .cancelled
                .filter(|_| shown.cancelled != heard.cancelled)
                .map(str::to_owned),
        ]
        .into_iter()
        .flatten()
        .reduce(|notice, cancelled| format!("{notice} {cancelled}"));
        // A failure and refused input that change together, or that presses answer together,
        // are one alert, as the alert node holds one.
        let alert = [
            shown.error.clone().filter(|_| {
                shown.error != heard.error || shown.presses.error != heard.presses.error
            }),
            shown
                .issue
                .filter(|_| {
                    shown.issue != heard.issue || shown.presses.issue != heard.presses.issue
                })
                .map(|issue| issue.message().to_owned()),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
        // The frame being drawn has drawn the window's announcements already.
        window.defer(cx, move |window, cx| {
            if let Some(notice) = notice {
                copy_feedback::announce(window, cx, notice);
            }
            if !alert.is_empty() {
                copy_feedback::alert(window, cx, alert);
            }
        });
    }
    /// The line in the failure's place, `failure` or `cancelled` its id: the last request's
    /// failure in the warning colour, or, where its Cancel stopped it, that it was cancelled
    /// ([`RequestKind::cancelled`]), muted as the notice is, since the person asked for it.
    /// Both are statuses; the window says the failure as an alert and the cancel politely
    /// ([`Self::announce`]).
    fn outcome(
        &self,
        failure: &'static str,
        cancelled: &'static str,
        cx: &App,
    ) -> Option<native_accessibility::StaticText<Stateful<Div>>> {
        let p = palette(cx);
        match (&self.error, self.cancelled) {
            (Some(error), _) => Some(label(failure, error.clone()).text_color(rgb(p.warning))),
            (None, Some(line)) => Some(label(cancelled, line).text_color(rgb(p.muted))),
            (None, None) => None,
        }
        .map(|line| line.role(Role::Status))
    }
    /// The panel's top line while a conversation is open: `lines`, its notice, its failure or
    /// cancel and refused input, then `busy`, the busy row with its Cancel, on the panel's
    /// surface above the body with a divider under them, where they stay in view, rather than
    /// among the content under its toolbar. As the line appears, changes height or clears, the
    /// body's scroll moves by as much, in the same frame and before the body places its
    /// content, so nothing drawn in the conversation moves: what the line takes from the
    /// body's top is still the body's, reached by scrolling up, and when the line clears the
    /// body takes it back with the scroll. The reveal the body last made keeps its place, so
    /// the shorter body reveals nothing anew. Among the content, a line appearing pushed the
    /// conversation, the draft being written included, down by its height. Where the content
    /// does not fill the body, there is nothing to scroll, and the conversation moves down by
    /// the line rather than lie under it. Text lines taller than half the body's room scroll
    /// within the line, so the body keeps room for the conversation, and the line's scrollbar
    /// cues the rest ([`crate::tags::ScrollCue`]); a long remedy was once cut there with no
    /// sign of more. The line's text and busy row keep the body's right inset, the scroll
    /// gutter its content always keeps ([`crate::tags::ScrollCue::gutter`]), with the line's
    /// own scrollbar drawn in it, so a line wraps as it did among the content: full width
    /// above the body, a long notice was 19 px wider pinned and could re-wrap as it moved
    /// there. Its divider spans the panel, as the body's track does.
    /// `stood` is whether the line stood above the body in the last frame too.
    fn pin_top_line(
        &self,
        lines: Vec<AnyElement>,
        busy: Option<AnyElement>,
        stood: bool,
        window: &Window,
        cx: &App,
    ) -> Div {
        let p = palette(cx);
        let drawn = self.review.top_line.clone();
        let scroll = self.review.body_scroll.clone();
        let revealed = self.review.revealed_focus.clone();
        // The text holds no control, so its rows keep no room for rings.
        let text = &self.review.top_line_scroll;
        let cue = crate::tags::ScrollCue::new(text, Pixels::ZERO);
        let gutter = crate::tags::ScrollCue::gutter();
        let line = (!lines.is_empty() || busy.is_some()).then(|| {
            div()
                .debug_selector(|| "github-top-line".into())
                .flex()
                .flex_col()
                .gap_2()
                .pb_2()
                .border_b_1()
                .border_color(rgb(p.border))
                .when(!lines.is_empty(), |line| {
                    line.child(
                        cue.list(
                            div()
                                .on_children_prepainted(cue.observe(|_, _, _| {}))
                                .id("github-top-line-scroll")
                                .debug_selector(|| "github-top-line-scroll".into())
                                .track_scroll(text)
                                .max_h(body_max_height(window) / 2.)
                                // The gutter, which the cue keeps too while the text
                                // overflows, its scrollbar in it.
                                .pr(gutter)
                                .overflow_y_scroll()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .children(lines),
                            "github-top-line-scrollbar",
                        ),
                    )
                })
                .children(busy.map(|busy| div().pr(gutter).child(busy)))
        });
        // Prepainted before the body, which reads its scroll after.
        div()
            .flex_shrink_0()
            .on_children_prepainted(move |line, _, _| {
                let height = line.first().map_or(Pixels::ZERO, |line| line.size.height);
                let before = drawn.replace(height);
                // Among the content last frame, the line took its room there, not the body's.
                let took = if stood { height - before } else { height };
                shift_body(&scroll, &revealed, height - before, took);
            })
            .children(line)
    }
    /// For the frame in which the top line moves back among the content from above the body,
    /// as the open conversation closes: an empty element prepainted before the body, which
    /// moves the body's scroll by the room the line takes there, read from this frame's layout
    /// of it (`laid`, [`LaidOut`]), less the height it stood above the body, so the
    /// conversation stays where it was drawn, as [`Self::pin_top_line`] does as it opens. The
    /// room is the line's height and the content's gap after it. Without it, the conversation
    /// moved by that difference as a reply closed: a pixel for a short line, the divider it no
    /// longer draws, and for a long line capped above the body all of its text the cap hid.
    fn unpin_top_line(&self, laid: std::rc::Rc<std::cell::Cell<Option<LayoutId>>>) -> Div {
        let drawn = self.review.top_line.clone();
        let scroll = self.review.body_scroll.clone();
        let revealed = self.review.revealed_focus.clone();
        let stood = drawn.get();
        div().on_children_prepainted(move |_, window, _| {
            // The content's `gap_2` after the line.
            let room = laid.get().map_or(Pixels::ZERO, |line| {
                window.layout_bounds(line).size.height + window.rem_size() * 0.5
            });
            drawn.set(room);
            // The body takes back the height the line stood above it.
            shift_body(&scroll, &revealed, room - stood, -stood);
        })
    }
    /// The panel's top line among the content, under the toolbar, while no conversation is
    /// open: `lines`, then `busy`, as one part of the content whose lines each start a block of
    /// the body. It records in `flowing` where it lies in the frame, from which the content
    /// records the room it takes there, its gap to the part after it included
    /// ([`review::ReviewState::top_line`]): as a reply opens and the line moves above the body
    /// ([`Self::pin_top_line`]), the body's scroll moves by only the difference, so the
    /// conversation below stays where it was drawn. With no room recorded there, the line
    /// moving above the body counted as a new line and scrolled the conversation up by its
    /// whole height.
    fn flow_top_line(
        &self,
        lines: Vec<AnyElement>,
        busy: Option<AnyElement>,
        flowing: std::rc::Rc<std::cell::Cell<Option<(Pixels, Pixels)>>>,
    ) -> Option<Div> {
        if lines.is_empty() && busy.is_none() {
            return None;
        }
        let blocks = self.review.body_blocks.clone();
        Some(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .on_children_prepainted(move |line, _, _| {
                    blocks.report(&line);
                    if let (Some(first), Some(last)) = (line.first(), line.last()) {
                        flowing.set(Some((first.top(), last.bottom())));
                    }
                })
                .children(lines)
                .children(busy),
        )
    }
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending && self.request.writing() {
            self.refuse("A GitHub action is in progress for this captured destination. Wait for the result, or choose Cancel and inspect GitHub before resubmitting.");
            cx.notify();
            return;
        }
        if let Some(control) = &self.control {
            control.cancel();
        }
        if let Some(control) = &self.local_control {
            control.cancel();
        }
        // A press the closing panel took ends with it, so a reopened list is drawn at rest.
        self.pressed.set(None);
        self.save_draft(window, cx);
        self.closed = true;
        self.heard = Heard::default();
        self.review.opened.set(false);
        // A reopened panel draws no line where it was, so it moves nothing for one.
        self.review.top_line_pinned.set(false);
        self.request_generation = self.request_generation.wrapping_add(1);
        self.pending = false;
        self.task = None;
        self.review.suspend_preparation();
        self.local_generation = self.local_generation.wrapping_add(1);
        self.local_loading = false;
        self.local_task = None;
        if let Some(owner) = self.owner.upgrade() {
            let entry = WarmPanel {
                owner: owner.entity_id(),
                worktree: self.repo.path().to_owned(),
                panel: cx.entity(),
                bytes: self.retained_review_bytes(cx),
            };
            cx.default_global::<WarmPanels>().retain(entry);
        }
        window.close_dialog(cx);
        if let Some(owner) = self.owner.upgrade() {
            let focus = owner.update(cx, |owner, _| owner.modal_return(window));
            focus.focus(window, cx);
        }
    }
    fn changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pull) = self
            .pull
            .as_ref()
            .and_then(|pull| self.destination(cx).ok().map(|repo| pull.capture(repo)))
        else {
            return;
        };
        let target = ComparisonTarget {
            worktree: self.repo.path().to_owned(),
            pull,
        };
        let repo = self.repo.clone();
        let cancellation = HistoryCancellation::default();
        self.local_control = Some(cancellation.clone());
        let stop = cancellation.clone();
        let sent = self.run(RequestKind::Compare, move |_| {
            let comparison = repo.compare_revisions(&target.pull.base.sha, &target.pull.head.sha, gitturtle_core::ComparisonMode::SinceBranching, &cancellation)?;
            Ok((target.worktree, comparison))
        }, |result, this, window, cx| match result {
            Ok((path, comparison)) => {
                this.close(window, cx);
                let _ = this.owner.update(cx, |owner, cx| {
                    if owner.path.as_ref() == Some(&path) && owner.page == AppPage::Repository { owner.show_revision_comparison(comparison, window, cx); }
                });
            },
            Err(error) => this.error = Some(format!("PR changes need their captured head, base, and common ancestor locally. Use an explicit Fetch if needed, then reopen the PR. {error:#}")),
        }, window, cx);
        // The comparison reads through its own cancellation, which the
        // panel's Cancel stops; the operation bar's Cancel stops it too.
        if sent {
            let _ = self
                .owner
                .update(cx, |owner, _| owner.cancel_with_operation(stop));
        }
    }
    fn discard_draft(&mut self, draft: Draft, window: &mut Window, cx: &mut Context<Self>) {
        let key = draft.key();
        let response = self.owner.update(cx, |owner, _| {
            owner
                .preferences_writer
                .submit(move || drafts::remove_exact(&draft))
        });
        let Ok(response) = response else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            let result = response.await;
            let _ = this.update_in(cx, |this, _, cx| {
                match result {
                    Ok(Ok(())) => {
                        this.saved.retain(|draft| draft.key() != key);
                        this.notice =
                            Some("The selected saved draft was discarded locally.".into());
                    }
                    _ => this.refuse(
                        "Could not discard the selected draft; its stored text was preserved.",
                    ),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
impl Render for Panel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.announce(window, cx);
        let p = palette(cx);
        let pending = self.pending;
        let scroll = self.review.body_scroll.clone();
        let observed = self.review.revealed_focus.clone();
        let show_editor =
            self.create || (self.pull.is_some() && self.review.section == Section::Review);
        // The room a focused control's ring takes outside its edge.
        let room = appearance::button_ring_room(cx);
        let blocks = self.review.body_blocks.clone();
        // Where the line holding the open PR's title starts, for showing the PR as it opens.
        let title_line = std::rc::Rc::new(std::cell::Cell::new(px(0.)));
        let line = title_line.clone();
        // The panel's top line: its notice, the failure or cancel that answers there, refused
        // input, then the busy row with its Cancel. While a conversation is open it stands
        // above the body ([`Self::pin_top_line`]); otherwise among the content, under the
        // toolbar.
        let pinned = self.conversations.active.is_some();
        // A new line, or a press answered again, shows from its first sentence: a new long
        // failure once opened where the last one had been scrolled to.
        let said = Heard {
            notice: self.notice.clone(),
            error: self.error.clone(),
            cancelled: self.cancelled,
            issue: self.draft_issue,
            presses: self.presses,
        };
        if said != self.top_line_said {
            self.review
                .top_line_scroll
                .set_offset(point(px(0.), px(0.)));
            self.top_line_said = said;
        }
        let mut lines = Vec::new();
        lines.extend(self.notice.as_ref().map(|notice| {
            label("github-notice", notice.clone())
                .debug_selector(|| "github-notice".into())
                .ml(text_inset())
                .role(Role::Status)
                .text_color(rgb(p.muted))
                .into_any_element()
        }));
        lines.extend(
            self.outcome("github-error", "github-cancelled", cx)
                .filter(|_| !self.answers_in_reply())
                .map(|line| line.ml(text_inset()).into_any_element()),
        );
        lines.extend(self.draft_issue.map(|issue| {
            label("github-draft-validation", issue.message())
                .ml(text_inset())
                .role(Role::Status)
                .text_color(rgb(p.warning))
                .into_any_element()
        }));
        // Pinned, the row is always in view, so it is no part the body reveals.
        let busy = pending.then(|| {
            if pinned {
                div()
            } else {
                self.part("busy", div(), cx)
            }
            .flex()
            .items_center()
            .gap_2()
            .pl(text_inset())
            .child(
                label("github-busy", self.request.label())
                    .role(Role::Status)
                    .live_region(gpui::accesskit::Live::Polite, true),
            )
            .child(
                button("github-cancel", "Cancel", "", false)
                    .track_focus(self.cancel_request.handle())
                    .debug_selector(|| "github-cancel".into())
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(control) = &this.control {
                            control.cancel();
                        }
                        if let Some(control) = &this.local_control {
                            control.cancel();
                        }
                        this.notice = Some(this.request.requested().into());
                        cx.notify();
                    })),
            )
            .into_any_element()
        });
        // Where the line lies among the content this frame, its top and bottom, if it does.
        let flowing = std::rc::Rc::new(std::cell::Cell::new(None));
        // A line the last frame stood above the body moves back among the content as the
        // conversation closes, and an element before the body moves the body's scroll by the
        // difference, read from where this frame lays the line out ([`Self::unpin_top_line`]).
        let stood = self.review.top_line_pinned.replace(pinned);
        let unpinned = stood && !pinned;
        let (flowing_line, top_line) = if pinned {
            (
                None,
                Some(self.pin_top_line(lines, busy, stood, window, cx)),
            )
        } else if unpinned {
            let laid = std::rc::Rc::new(std::cell::Cell::new(None));
            let ahead = self.unpin_top_line(laid.clone());
            let line = self
                .flow_top_line(lines, busy, flowing.clone())
                .map(|line| {
                    LaidOut {
                        child: line.into_any_element(),
                        laid,
                    }
                    .into_any_element()
                });
            (line, Some(ahead))
        } else {
            self.review.top_line.set(Pixels::ZERO);
            let line = self.flow_top_line(lines, busy, flowing.clone());
            (line.map(IntoElement::into_any_element), None)
        };
        // Each part of the content starts a block of the body ([`review::Blocks`]), and the
        // line among it records the room it takes there, up to the part after it
        // ([`Self::flow_top_line`]).
        let starts = blocks.starts();
        let drawn = self.review.top_line.clone();
        let parts = move |children: Vec<Bounds<Pixels>>, window: &mut Window, cx: &mut App| {
            if let Some((top, bottom)) = flowing.get()
                && let Some(next) = children.iter().find(|part| part.top() >= bottom)
            {
                drawn.set(next.top() - top);
            }
            starts(children, window, cx);
        };
        // The panel's content, its text one inset in from this edge. Each part of it starts a
        // block of the body, where a reveal may bring the body's top edge.
        let content = div().debug_selector(||"github-panel-content".into()).on_children_prepainted(parts).min_w_0().flex().flex_col().gap_2()
            .when(self.review.fixture,|element|element.child(self.part("banner",div(),cx).flex().flex_wrap().items_center().gap_2().py_2().px(text_inset()).rounded(px(6.)).bg(rgb(p.subtle))
                .child(label("github-fixture-mode","OFFLINE FIXTURE · native review · no network or credentials").debug_selector(||"github-fixture-mode".into()).min_w_0().text_color(rgb(p.muted)))
                .child(button("github-fixture-move",if self.review.fixture_moved{"Fixture head moved"}else{"Move fixture head"},"",false).debug_selector(||"github-fixture-move".into()).disabled(pending||self.review.fixture_moved).on_click(cx.listener(|this,_,_,cx|{this.review.fixture_moved=true;this.confirm=None;this.notice=Some("Fixture head moved remotely. This view and its draft retain the original commit. Submit to verify stale-head refusal, or Refresh PRs and reopen to inspect the new head.".into());cx.notify();})))))
            .child(self.part("toolbar",div(),cx).flex().flex_wrap().items_center().gap_2()
                // Focus moved onto the field, by a click, by Name repository below, which
                // reveals it even when it holds focus already, or by the panel reopening onto
                // it, scrolls it into view with its ring, as a Tab onto it does, and only where
                // it is not whole already. It is a part of its own, as each control in the row
                // is: the row can be taller than the body, as it was at 18 pt in a 461 × 490
                // window while the body stopped short of the dialog's cap, and revealing the
                // whole row scrolled a field already whole, and the banner above it out of
                // view.
                .child(self.part("destination",div(),cx).debug_selector(||"github-destination".into()).w(appearance::ui_size(280.)).min_w(px(190.)).child(review::one_line()).child(review::starts_block()).child(div().on_children_prepainted(self.reveal_field_on_focus(self.destination.read(cx).focus_handle(cx))).child(Input::new(&self.destination).aria_label("GitHub destination owner and repository").disabled(pending))))
                .child(self.part("refresh",div(),cx).child(review::starts_block()).child(button("github-refresh",if self.review.fixture{"Refresh fixture PRs"}else{"Refresh PRs"},"refresh",false).debug_selector(||"github-refresh".into()).disabled(pending).on_click(cx.listener(|this,_,window,cx|this.refresh(1,window,cx)))))
                .child(self.part("create",div(),cx).child(review::starts_block()).child(button("github-create","New pull request","plus",false).debug_selector(||"github-create".into()).disabled(pending||self.review.fixture).on_click(cx.listener(|this,_,window,cx|this.create(window,cx)))))
                .child(div().flex_1()).child(self.part("account",div(),cx).child(review::starts_block()).child(button("github-account-options",self.account.as_ref().map(|account|format!("Account · {account}")).unwrap_or_else(||"Connect account".into()),"",self.review.connection_open).debug_selector(||"github-account-options".into()).toggled(self.review.connection_open).disabled(pending).on_click(cx.listener(|this,_,_,cx|{this.review.connection_open= !this.review.connection_open;cx.notify();})))))
            // The connection pane. Each of its controls is a part of its own, as the toolbar's
            // are, so Tab onto one shows that control whole, and its lines and its controls, as
            // the toolbar's do, start blocks of the body, so the reveal can bring the pane's
            // first line, or the row of controls it wraps to, to the body's top with it:
            // revealed as one part, the pane, taller than the body at 18 pt in a 461 × 490
            // window, showed its foot and left the help's second line cut at the body's top.
            // Its first line names the local worktree on one line, the path elided in its
            // middle where it is longer than the line ([`path_label`]). The pane is a stop of
            // its own before its controls, its ring on its edge as the conversations pane's
            // is, so Tab shows its first line and its help from its top: the stops of its
            // controls alone could not show them with every line whole in a short body, as at
            // 18 pt in a 461 × 490 window, where macOS lays the help out in five lines (and
            // Linux from 470 px wide), and no stop showed the worktree line or the help's
            // first line at all.
            .when(self.review.connection_open,|element|element.child(self.ringed_part("connection",div(),&self.review.connection_focus,cx).child(div().id("github-connection").role(Role::Group).aria_label("GitHub connection: local worktree and sign-in").track_focus(&self.review.connection_focus).map(bordered_pane).border_1().border_color(rgb(if self.ringed(&self.review.connection_focus,cx){p.accent}else{p.border})).rounded(px(6.))
                .child(div().flex().flex_col().gap_2().on_children_prepainted(self.review.body_blocks.starts())
                .child(path_label("github-worktree",&self.worktree).text_color(rgb(p.muted)))
                .child(label("github-connection-help",connection_help()).text_color(rgb(p.muted)))
                .child(div().flex().flex_wrap().gap_2()
                    .child(self.part("connect",div(),cx).child(review::starts_block()).child(button("github-connect","Connect GitHub CLI account","",false).debug_selector(||"github-connect".into()).disabled(pending||self.review.fixture).on_click(cx.listener(|this,_,window,cx|this.connect(window,cx)))))
                    .child(self.part("sign-in-help",div(),cx).child(review::starts_block()).child(copy_button(copy_feedback::Mark::new("github-sign-in-help",SIGN_IN_COMMAND),"Copy sign-in command","",cx).on_click(|_,window,cx|copy_feedback::copy(window,cx,Some(copy_feedback::Mark::new("github-sign-in-help",SIGN_IN_COMMAND)),"sign-in command",SIGN_IN_COMMAND.into()))))
                    .child(self.part("disconnect",div(),cx).child(review::starts_block()).child(button("github-disconnect","Disconnect GitTurtle","",false).debug_selector(||"github-disconnect".into()).disabled(pending||self.review.fixture).on_click(cx.listener(|this,_,window,cx|this.disconnect(window,cx))))))))))
            .children(flowing_line)
            .when(!self.create && (self.review.show_list||self.pull.is_none()),|element|element
                // The list scrolls, so it clips its rows on both axes. Its full-width rows keep
                // their rings' room inside that clip: its sides give the room back through their
                // margin, so the rows' text keeps the panel's edge, and above the first row and
                // below the last the room is the list's own (a negative margin there collapsed
                // the panel's scrolling body to its padding).
                .child(review::clipped(crate::native_accessibility::clips_children(self.part("pr-list",div(),cx).id("github-pr-list").debug_selector(||"github-pr-list".into()).flex_shrink_0().max_h(px(200.)+room*2.).px(room).mx(-room).py(room).overflow_y_scroll().flex().flex_col().gap_1()
                    .children(self.rows.iter().map(|pull|{let pull=pull.clone();let selected=self.pull.as_ref().is_some_and(|p|p.number==pull.number);let number=pull.number;let name=format!("#{} · {}",pull.number,pull.title);let context=format!("{} · {} → {} · {}",if pull.draft{"Draft"}else{"Open"},pull.head.branch,pull.base.branch,pull.user.login);
                        // A list row, as the Files list's rows are: it rests on the panel surface,
                        // the selected one on the selected surface, and lifts to the row hover under
                        // both its lines, which open the PR alike. The title Button spans it, its
                        // label at the panel's text inset, and the context line below, outside the
                        // Button, takes the same inset as its padding and the row's foot below it,
                        // so the whole lit row opens the PR (as a margin and the row's own padding,
                        // they left 10 px each side and the foot unclickable). A press anywhere on
                        // the row darkens it whole to the selected surface, as list rows do. The
                        // title draws no fill of its own, at rest, hovered, pressed or selected,
                        // so the row's surface shows through it: the row lifts and darkens once,
                        // under both lines, where the title once pressed its own line alone.
                        // The title is a part of the body of its own, named by the PR's number:
                        // Tab onto it shows the title with its ring, and the row's line keeps
                        // its context line in view with it wherever a place cuts no line.
                        // Revealed as the list, with the room the list keeps below its last
                        // row, a reveal found no such place at 17 pt in a 461 × 490 window and
                        // left the next line cut at the body's edge, though one showed the row.
                        let hover=p.row_hover(selected);let open=pull.clone();
                        let flat=gpui_kit::component::button::ButtonVariant::Custom(gpui_kit::component::button::ButtonCustomVariant::new(cx).foreground(rgb(p.control_label()).into()));
                        let title=crate::button_frame(("github-pr",number),&name,"",false).with_variant(flat).selected(selected);
                        let pressed=!pending&&self.pressed.get()==Some(number);let (down,up,out)=(self.pressed.clone(),self.pressed.clone(),self.pressed.clone());
                        div().id(("github-pr-card",pull.number)).debug_selector(move||format!("github-pr-card-{number}")).child(review::one_line()).flex_shrink_0().rounded(px(7.)).bg(rgb(if selected||pressed{p.selected}else{p.panel})).when(!pending&&!pressed,|row|row.hover(move|row|row.bg(rgb(hover))))
                            .when(!pending,|row|row.capture_any_mouse_down(move|event,window,_|if event.button==MouseButton::Left{down.set(Some(number));window.refresh();}).capture_any_mouse_up(move|_,window,_|if up.take().is_some(){window.refresh();}).on_mouse_up_out(MouseButton::Left,move|_,window,_|if out.take().is_some(){window.refresh();})).flex().flex_col()
                            .child(self.part(format!("pr-{number}"),div(),cx).child(title.key_context("GitTurtleGithubButton").debug_selector(move||format!("github-pr-{number}")).w_full().accessibility_label(name.clone()).tooltip(name.clone()).child(row_text(format!("github-pr-title-{number}"),name)).disabled(pending).on_click(cx.listener(move|this,_,window,cx|this.inspect(pull.clone(),window,cx)))))
                            .child(native_accessibility::static_text(div().id(("github-pr-context",number)),context.clone()).flex().px(text_inset()).pb_1().text_size(appearance::ui_text(12.)).text_color(rgb(p.muted)).tooltip({let context=context.clone();move|window,cx|Tooltip::new(context.clone()).build(window,cx)}).when(!pending,|line|line.on_click(cx.listener(move|this,_,window,cx|this.inspect(open.clone(),window,cx)))).child(row_text(format!("github-pr-context-{number}"),context)))
                    }))
                    // One line with the next step: a repository with no GitHub remote names
                    // the destination first, and one with a destination reads its PRs with
                    // Refresh PRs, above. Opening the panel itself contacts nothing. Name
                    // repository moves to a name field, not a search, so it draws no glyph.
                    // The empty state, its line and action, is one line of the body: the body's
                    // reveals leave neither cut at its edge.
                    .when(self.rows.is_empty(),|element|element.child(div().py_5().px(text_inset()).child(div().child(review::one_line()).child(if self.local_loaded&&self.destination.read(cx).value().trim().is_empty(){
                        EmptyState::new("github-list-empty","No GitHub remote · Name the repository to load its pull requests").tooltip("Enter the destination as owner/repository or a GitHub.com remote URL. Opening this panel reads local settings and drafts only.")
                            .action(crate::empty_state::action("github-choose-destination","Name repository","").disabled(pending).on_click(cx.listener(|this,_,window,cx|{let focus=this.destination.read(cx).focus_handle(cx);window.focus(&focus,cx);
                                // The field may hold focus already, as it does once the panel opens:
                                // the next frame reveals it all the same.
                                *this.review.revealed_focus.borrow_mut()=Default::default();cx.notify();})))
                    }else{
                        EmptyState::new("github-list-empty","No pull requests shown · Refresh PRs loads the open ones").tooltip("Opening this panel reads local settings and drafts only.")
                    }.block(cx))))))))
                // A row that cannot hold its controls wraps them below, whole. A label after a
                // Button takes the panel's text inset, which the Button gives back, so beside it
                // nothing moves and on a line of its own the label starts at the panel's edge
                // for text.
                .child(self.part("pr-pager",div(),cx).flex().flex_wrap().items_center().gap_2().child(button("github-previous","Previous PR page","",false).debug_selector(||"github-previous".into()).mr(-text_inset()).disabled(pending||self.page<=1).on_click(cx.listener(|this,_,window,cx|this.refresh(this.page-1,window,cx))))
                    .child(phrases("github-page",format!("Page {} · up to {} PRs",self.page,github::PAGE_SIZE)).min_w_0().ml(text_inset()).text_color(rgb(p.muted)))
                    .child(button("github-next","Next PR page","",false).debug_selector(||"github-next".into()).disabled(pending||!self.has_next).on_click(cx.listener(|this,_,window,cx|this.refresh(this.page+1,window,cx))))))
            .when_some(self.pull.as_ref(),|element,pull|element
                // The PR's header and its section tabs, shown from the title's line as the PR
                // opens.
                .child(div().flex().flex_col().gap_2().on_children_prepainted(self.reveal_opened(title_line,cx)).child(self.part("pull-header",div(),cx).flex().flex_col().gap_2()
                    .child(div().flex().flex_wrap().items_center().gap_2().on_children_prepainted(move|children,_,_|{
                        // The title's line: the Button's where the title stands beside it, its
                        // own where it wraps below.
                        if let [back,title]=children[..]{line.set(if title.top()<back.bottom(){back.top().min(title.top())}else{title.top()});}
                    }).child(button("github-back-list",if self.review.show_list{"Hide PR list"}else{"Pull requests"},"arrow-left",false).debug_selector(||"github-back-list".into()).mr(-text_inset()).disabled(pending).on_click(cx.listener(|this,_,window,cx|{this.save_draft(window,cx);this.review.show_list= !this.review.show_list;cx.notify();})))
                        .child(label("github-pull-title",format!("#{} · {}",pull.number,pull.title)).debug_selector(||"github-pull-title".into()).min_w_0().ml(text_inset()).font_weight(FontWeight::SEMIBOLD).text_size(appearance::ui_text(15.))))
                    .child(label("github-branch-context",format!("{} → {} · {}",pull.head.branch,pull.base.branch,if pull.draft{"Draft PR"}else{"Open PR"})).ml(text_inset()).text_color(rgb(p.muted))))
                .child(self.render_tabs(cx)))
                .when(self.review.section==Section::Overview,|element|element
                    .child(label("github-captured-identities",format!("Before · {}\nAfter  · {}",pull.base.sha,pull.head.sha)).ml(text_inset()).text_color(rgb(p.muted)))
                    .when_some(self.description.as_ref(),|element,description|element.child(self.part("description",div(),cx).relative().h(px(120.)).flex_shrink_0()
                        // Each row of the description its viewport shows is a line of the body.
                        .child(review::editor_rows(description))
                        .child(crate::editor_find::Editor::new(description).readonly(true).h_full().aria_label("Pull request description, exact Markdown source"))))
                    .when_some(self.status.as_ref(),|element,status|element.child(div().py_3().px(text_inset()).bg(rgb(p.subtle)).rounded(px(6.)).flex().flex_col().gap_1()
                        .child(label("github-review-status",format!("Reviews · {}",if status.reviews.is_empty(){"None reported".into()}else{status.reviews.join(" · ")})))
                        .child(label("github-check-status",format!("Checks · {}{}",if status.checks.is_empty(){"None reported".into()}else{status.checks.join(" · ")},if status.incomplete{" · Additional status entries are outside this preview"}else{""})))))
                    .child(self.render_threads(None,window,cx))
                    .child(self.part("open-changes",div(),cx).flex().child(button("github-open-changes","Open captured local comparison","",false).debug_selector(||"github-open-changes".into()).disabled(pending).on_click(cx.listener(|this,_,window,cx|this.changes(window,cx))))))
                .when(self.review.section==Section::Files,|element|element.child(self.render_files(window,cx)))
                .when(self.review.section==Section::Review,|element|element.child(self.render_collected(cx))))
            .when(self.create,|element|element
                .child(button("github-create-back","Pull requests","arrow-left",false).disabled(pending).on_click(cx.listener(|this,_,window,cx|{this.save_draft(window,cx);this.create=false;this.confirm=None;this.review.show_list=true;this.focus_visible_section(window,cx);cx.notify();})))
                .child(label("github-create-fields","New pull request · published head → destination base").font_weight(FontWeight::SEMIBOLD))
                // Each one-line field is one line of the body, which its reveals leave whole.
                .child(div().child(review::one_line()).child(Input::new(&self.title).aria_label("Pull request title").disabled(pending)))
                .child(div().flex().gap_2().child(div().flex_1().child(review::one_line()).child(Input::new(&self.head).aria_label("Published head branch").disabled(pending))).child(div().flex_1().child(review::one_line()).child(Input::new(&self.base).aria_label("Destination base branch").disabled(pending))))
                .child(div().flex().gap_2().children([(true,"Draft"),(false,"Ready")].map(|(draft,name)|button(name,name,"",self.draft==draft).toggled(self.draft==draft).disabled(pending).on_click(cx.listener(move|this,_,window,cx|{this.draft=draft;this.confirm=None;this.save_draft(window,cx);}))))
                    .child(button("github-load-templates","Load local base templates","",false).disabled(pending).on_click(cx.listener(|this,_,window,cx|this.load_templates(window,cx)))))
                .children(self.templates.iter().enumerate().map(|(index,template)|{let template=template.clone();button(("github-template",index),format!("Append {} · {}",template.path,&template.revision[..8]),"",false).disabled(pending).on_click(cx.listener(move|this,_,window,cx|{let body=format!("{}{}{}",this.body.read(cx).value(),if this.body.read(cx).value().is_empty(){""}else{"\n\n"},template.body);this.body.update(cx,|input,cx|input.set_value(body,window,cx));this.save_draft(window,cx);} ))})))
            .when(show_editor,|element|element
                .when(!self.create,|element|element.child(self.part("events",div(),cx).flex().flex_wrap().gap_1()
                    .children([ReviewEvent::Comment,ReviewEvent::Approve,ReviewEvent::RequestChanges].map(|event|button(event.label(),event.label(),"",!self.discussion&&self.event==event).toggled(!self.discussion&&self.event==event).disabled(pending).on_click(cx.listener(move|this,_,window,cx|{this.discussion=false;this.event=event;this.confirm=None;this.save_draft(window,cx);}))))
                    .child(button("github-discussion","Discussion only","",self.discussion).toggled(self.discussion).disabled(pending||!self.review.comments.is_empty()||self.review.composing.is_some()).on_click(cx.listener(|this,_,_,cx|{this.discussion=true;this.confirm=None;cx.notify();})))))
                .child(div().debug_selector(||"github-review-summary".into()).on_children_prepainted(self.reveal_field_on_focus(self.body.read(cx).focus_handle(cx))).child(review::editor_rows(&self.body)).child(Textarea::new(&self.body).h(appearance::ui_size(130.)).aria_label(if self.create{"Pull request description"}else{"Review summary"}).disabled(pending)))
                // The draft status beside Review submission… where it fits there whole, at the
                // panel's text inset; otherwise on a line of its own below it, wrapping, so its
                // remedy stays readable and the button stays inside the panel (at 18 pt a failed
                // save's status pushed the button past the panel's right edge).
                .child(self.part("submit",div(),cx).flex().flex_wrap_reverse().items_center().gap_2().child(label("github-draft-status",self.save_status.clone()).debug_selector(||"github-draft-status".into()).min_w_0().mx(text_inset()).role(Role::Status).polite().text_color(rgb(p.muted)))
                    .child(button("github-review-submit","Review submission…","",true).debug_selector(||"github-review-submit".into()).ml_auto().disabled(pending).on_click(cx.listener(|this,_,window,cx|this.review_submission(window,cx))))))
            .child(self.render_reply_composer(cx))
            .when_some(self.confirm.as_ref(),|element,confirmation|element.child(div().debug_selector(||"github-confirmation".into()).on_children_prepainted(self.reveal_on_focus(self.conversations.confirm_focus.clone())).track_focus(&self.conversations.confirm_focus).tab_stop(true).map(bordered_pane).border_1().border_color(rgb(p.accent)).rounded(px(7.)).flex().flex_col().gap_2()
                .child(label("github-confirm-destination",format!("{}\nAccount · {}",confirmation.action.destination(),confirmation.account)).font_weight(FontWeight::SEMIBOLD))
                .child(review::clipped(div().id("github-confirm-text-scroll").debug_selector(||"github-confirm-text-scroll".into()).flex_shrink_0().max_h(px(180.)).overflow_y_scroll().child(label("github-confirm-text",match &confirmation.action{Action::Create(pull)=>format!("{}\n\n{}",pull.title,pull.body),Action::Comment{body,..}|Action::Reply{body,..}=>body.clone(),Action::SetThreadResolved{resolved,..}=>if *resolved{"Resolve this conversation"}else{"Reopen this conversation"}.into(),Action::Review(review)=>format!("{} review · {}\n\n{}{}",review.event.label(),crate::counted(review.comments.len(),"inline comment","inline comments"),review.body,review.comments.iter().map(|comment|format!("\n\n{}\n{}",github::review::position_label(comment),comment.body)).collect::<String>())}))))
                .child(label("github-confirm-guidance","Send performs one outbound action with these exact captured targets and comments. Inspect the destination before repeating an uncertain attempt.").text_color(rgb(p.muted)))
                .child(div().flex().gap_2().child(button("github-confirm-send",if self.review.fixture{"Send to offline fixture"}else{"Send to GitHub"},"",true).disabled(pending).on_click(cx.listener(|this,_,window,cx|this.submit(window,cx))))
                    .child(button("github-edit-submission","Keep editing","",false).disabled(pending).on_click(cx.listener(|this,_,window,cx|{this.confirm=None;this.focus_visible_section(window,cx);cx.notify();}))))))
            .child(self.part("recovery",div(),cx).flex().flex_wrap().items_center().gap_2().child(button("github-recovery-toggle",format!("Recover drafts · {}",self.saved.len()),"",self.review.recovery_open).debug_selector(||"github-recovery-toggle".into()).mr(-text_inset()).toggled(self.review.recovery_open).on_click(cx.listener(|this,_,_,cx|{this.review.recovery_open= !this.review.recovery_open;cx.notify();})))
                .child(phrases("github-passive-policy",if cfg!(target_os="macos"){"Drafts stay on this Mac · outbound actions are explicit"}else{"Drafts stay on this computer · outbound actions are explicit"}).min_w_0().ml(text_inset()).text_color(rgb(p.muted))))
            .when(self.review.recovery_open,|element|element
                .child(label("github-saved-heading","Captured drafts · Copy includes all inline text and original positions; older heads are never remapped").ml(text_inset()).text_color(rgb(p.muted)))
                .child(review::clipped(crate::native_accessibility::clips_children(div().id("github-saved-drafts").debug_selector(||"github-saved-drafts".into()).flex_shrink_0().max_h(px(160.)).overflow_y_scroll().flex().flex_col().gap_2().children(self.saved.iter().filter(|draft| !matches!(draft,Draft::Reply(reply) if reply.body.is_empty())).take(128).enumerate().map(|(index,draft)|{let draft=draft.clone();let discard=draft.clone();let mark=copy_feedback::Mark::new(("github-copy-draft",index),draft.key());let copy_mark=mark.clone();div().flex_shrink_0().flex().flex_col().gap_1().child(label(("github-draft-identity",index),match &draft {Draft::Reply(reply)=>format!("Reply · {} #{} · {} · {}",reply.target.pull.repository.label(),reply.target.pull.number,reply.target.path,reply.target.account),_=>draft.key()}).ml(text_inset()).text_color(rgb(p.muted)))
                    .child(div().flex().gap_2().child(copy_button(mark,"Copy complete draft", "", cx).on_click(move|_,window,cx|copy_feedback::copy(window,cx,Some(copy_mark.clone()),"draft",draft.recovery_text())))
                        .child(button(("github-discard-draft",index),"Discard saved draft","",false).disabled(pending).on_click(cx.listener(move|this,_,window,cx|this.discard_draft(discard.clone(),window,cx)))))})))))
                .when(!self.attempts.is_empty(),|element|element.child(label("github-attempt-heading","Previous outbound attempts · inspect the destination before repeating").ml(text_inset()))
                    .child(review::clipped(crate::native_accessibility::clips_children(div().id("github-attempt-list").debug_selector(||"github-attempt-list".into()).flex_shrink_0().max_h(px(100.)).overflow_y_scroll().flex().flex_col().gap_1().children(self.attempts.iter().rev().take(10).enumerate().map(|(index,attempt)|label(("github-attempt",index),format!("{}: {}",attempt.destination,attempt.outcome)).ml(text_inset()).flex_shrink_0())))))));
        // The body scrolls inside the room its controls' focus rings take, which its sides
        // give back through their margin; above and below, the dialog's clip of its body
        // allows no margin, so the room is the body's own. While the body overflows, its
        // scrollbar shows the rest is there. Its content keeps the scrollbar's gutter whether
        // or not it overflows, so a section tab, a PR opening or a reply's composer, which
        // change what it holds, never move its right edge: kept only while the body
        // overflowed, the gutter moved that edge 19 px between Overview and Files.
        let cue = crate::tags::ScrollCue::new(&scroll, room);
        // Before the content: collects the lines it draws for the body's reveals, until the
        // body's own listener keeps them ([`review::Blocks::finish`]).
        let collect = blocks.collect_lines();
        let body = cue.list(
            div()
                .on_children_prepainted(cue.observe(move |_, window, cx| {
                    review::record_shown(
                        &observed,
                        &scroll,
                        |context| context.contains(review::BODY),
                        window,
                        cx,
                    );
                    blocks.finish();
                }))
                .key_context(review::BODY)
                .id("github-panel")
                .debug_selector(|| "github-panel".into())
                .track_scroll(&self.review.body_scroll)
                .min_w_0()
                .min_h_0()
                .max_h(body_max_height(window))
                .p(room)
                .pr(room + crate::tags::ScrollCue::gutter())
                .mx(-room)
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .child(collect)
                .child(content),
            "github-panel-scrollbar",
        );
        match top_line {
            // The line and the body share the body's height, the body taking what the line
            // leaves; in the frame the line moves back among the content, the empty element
            // that moves the body's scroll stands here instead.
            Some(top_line) => div()
                .min_w_0()
                .max_h(body_max_height(window))
                .flex()
                .flex_col()
                .child(top_line)
                .child(body.min_h_0())
                .into_any_element(),
            None => body.into_any_element(),
        }
    }
}

/// Submits a request of `kind` over `app`'s repository, as the panel sends
/// one, that answers at once once the operation executor runs it, for a
/// test of what the window says while a request runs: hold the executor
/// first to keep it running.
#[cfg(test)]
pub(crate) fn submit_test_request(
    app: &mut GitTurtle,
    kind: RequestKind,
    window: &mut Window,
    cx: &mut Context<GitTurtle>,
) {
    let repository = app.path.clone().expect("a repository");
    let submitted = app.submit_github_request(kind, repository, |_| anyhow::Ok(()), window, cx);
    assert!(submitted.is_ok(), "{kind:?}: nothing else runs");
}

/// Opens the panel over `app`'s repository on the offline review fixture,
/// listing its two PRs with the first one open, as the native review shows
/// it (`width_walk`). The caller settles the window.
#[cfg(test)]
pub(crate) fn open_offline_fixture(app: &Entity<GitTurtle>, window: &mut Window, cx: &mut App) {
    open_offline_fixture_with(app, window, cx, |_| {});
}

/// [`open_offline_fixture`], `prepare` changing the panel before it opens; the panel.
#[cfg(test)]
fn open_offline_fixture_with(
    app: &Entity<GitTurtle>,
    window: &mut Window,
    cx: &mut App,
    prepare: impl FnOnce(&mut Panel),
) -> Entity<Panel> {
    let repo = app.read(cx).repository.clone().unwrap();
    let panel = cx.new(|cx| Panel::new(app.downgrade(), repo, "main".into(), window, cx));
    let pull = github::review::fixture_pull(false);
    let mut other = pull.clone();
    other.number = 43;
    panel.update(cx, |panel, cx| {
        panel.rows = vec![pull.clone(), other];
        panel.review.fixture = true;
        panel.account = Some("offline-reviewer".into());
        panel.pull = Some(pull);
        panel.notice = Some("Fixture PRs refreshed.".into());
        prepare(panel);
        panel.close(window, cx);
    });
    app.update(cx, |app, cx| app.open_github(window, cx));
    panel
}

/// [`open_offline_fixture`] on its PR's Overview with every bordered pane it draws: the
/// connection pane open, the conversations read and a reply to the first one open and
/// reviewed, so its confirmation shows. For the layout grid; the body's scroll.
#[cfg(test)]
pub(crate) fn open_offline_fixture_panes(
    app: &Entity<GitTurtle>,
    window: &mut Window,
    cx: &mut App,
) -> ScrollHandle {
    let identity = github::review::fixture_pull(false)
        .capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
    let page = Client(UiTransport::Fixture { moved: false })
        .review_threads(&identity, None, &OperationControl::default())
        .unwrap();
    let target = page.items[0].target.clone();
    let reply = target.clone();
    let panel = open_offline_fixture_with(app, window, cx, move |panel| {
        panel.review.show_list = false;
        panel.review.section = Section::Overview;
        panel.review.connection_open = true;
        panel.receive_threads(page, None);
        panel.conversations.active = Some(reply);
    });
    // Opening clears a confirmation.
    panel.update(cx, |panel, cx| {
        panel.confirm = Some(Confirmation {
            account: target.account.clone(),
            action: Action::Reply {
                target,
                body: "Fixture reply".into(),
            },
        });
        cx.notify();
    });
    panel.read(cx).review.body_scroll.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::component::Root;
    use std::{cell::RefCell, rc::Rc};

    fn entered_fields() -> DraftFields {
        DraftFields {
            title: "Keep this title".into(),
            body: "Exact body\r\n  with spaces and café\n".into(),
            head: "topic/branch".into(),
            base: "main".into(),
        }
    }

    #[test]
    fn destination_typing_persists_only_settled_draft_and_preserves_prior_destinations() {
        use std::{
            collections::HashMap,
            sync::{Arc, Mutex},
        };

        let executor = operations::SerialExecutor::new("github-deferred-draft-test");
        let saver = commit_drafts::CoalescingSaver::default();
        let stored = Arc::new(Mutex::new(HashMap::<String, Draft>::new()));
        let persist = |draft: Draft| {
            let output = stored.clone();
            // Deliberately drop the UI reply: accepted writes must survive it.
            let _ = saver.queue_with(&executor, draft.key(), draft, move |batch| {
                output.lock().unwrap().extend(batch.clone());
                Ok(())
            });
            futures::executor::block_on(executor.submit(|| Ok(())))
                .unwrap()
                .unwrap();
        };
        let capture = |destination: &str| {
            capture_draft(
                destination,
                true,
                true,
                ReviewEvent::Comment,
                None,
                entered_fields(),
            )
            .unwrap()
        };
        let previous = capture("gitturtle-fixture/previous");
        let previous_key = previous.key();
        persist(previous);

        let mut deferred = DeferredDraft::default();
        let mut old_ticket = None;
        let destination = "milestone-qa";
        for end in 1..=destination.len() {
            let ticket = deferred.replace(capture(&format!(
                "gitturtle-fixture/{}",
                &destination[..end],
            )));
            if let Some(old) = old_ticket {
                assert!(
                    deferred.take(old).is_none(),
                    "superseded timer published a prefix"
                );
            }
            old_ticket = Some(ticket);
            assert_eq!(stored.lock().unwrap().len(), 1);
        }
        assert_eq!(
            finished_save_status(
                Ok(capture("gitturtle-fixture/milestone-qa")),
                deferred.is_pending(),
                true,
            ),
            "Saving local draft…",
            "an earlier successful save must not mark a pending edit saved",
        );
        let final_draft = deferred.take(old_ticket.unwrap()).unwrap();
        let final_key = final_draft.key();
        persist(final_draft);
        assert!(!deferred.is_pending());
        let saved = stored.lock().unwrap();
        assert_eq!(
            saved.len(),
            2,
            "intermediate destinations must not consume recovery slots"
        );
        assert!(saved.contains_key(&previous_key));
        let Draft::Pull(final_draft) = &saved[&final_key] else {
            unreachable!()
        };
        assert_eq!(
            final_draft.repository.label(),
            "gitturtle-fixture/milestone-qa"
        );
        assert_eq!(final_draft.title, entered_fields().title);
        assert_eq!(final_draft.body, entered_fields().body);
    }

    #[test]
    fn correcting_destination_captures_existing_text_and_distinguishes_missing_selection() {
        let fields = entered_fields();
        let capture = |destination, creating| {
            capture_draft(
                destination,
                creating,
                true,
                ReviewEvent::Comment,
                None,
                fields.clone(),
            )
        };
        for invalid in ["", "invalid destination", "owner"] {
            assert_eq!(capture(invalid, true).unwrap_err(), DraftIssue::Destination);
        }
        assert_eq!(
            capture("invalid destination", false).unwrap_err(),
            DraftIssue::Selection
        );
        assert_eq!(
            capture("fixture/repository", false).unwrap_err(),
            DraftIssue::Selection
        );
        let Draft::Pull(corrected) = capture("fixture/repository", true).unwrap() else {
            panic!("create form produces a pull request draft");
        };
        assert_eq!(corrected.repository.label(), "fixture/repository");
        assert_eq!(corrected.title, fields.title);
        assert_eq!(corrected.body, fields.body);
        assert_eq!(corrected.head, fields.head);
        assert_eq!(corrected.base, fields.base);
        assert!(corrected.draft);
        assert_eq!(corrected.expected_head, None);
        assert_eq!(corrected.expected_base, None);
    }

    #[test]
    fn save_completion_keeps_invalid_destination_and_failed_storage_unsaved() {
        assert_eq!(
            finished_save_status(Err(DraftIssue::Destination), false, true),
            DraftIssue::Destination.save_message(),
        );
        let capture = || {
            capture_draft(
                "fixture/repository",
                true,
                true,
                ReviewEvent::Comment,
                None,
                entered_fields(),
            )
        };
        assert_eq!(
            finished_save_status(capture(), true, true),
            "Saving local draft…"
        );
        assert_eq!(
            finished_save_status(capture(), false, true),
            "Draft saved locally"
        );
        assert_eq!(
            finished_save_status(capture(), true, false),
            "Draft save failed — keep this window open or copy the text"
        );
    }

    #[test]
    fn out_of_order_save_replies_cannot_replace_a_newer_failure_or_success() {
        let capture = || {
            capture_draft(
                "fixture/repository",
                true,
                true,
                ReviewEvent::Comment,
                None,
                entered_fields(),
            )
        };
        let mut status = SharedString::default();
        // The failing saver retains its unsaved batch; there is no scheduled
        // retry. An older success must not leave the form stuck on Saving.
        apply_save_completion(&mut status, 2, 2, capture(), true, false);
        let failure = status.clone();
        assert!(failure.contains("failed"));
        apply_save_completion(&mut status, 1, 2, capture(), true, true);
        assert_eq!(status, failure);

        apply_save_completion(&mut status, 3, 3, capture(), false, true);
        assert_eq!(status, "Draft saved locally");
        apply_save_completion(&mut status, 2, 3, capture(), false, false);
        assert_eq!(status, "Draft saved locally");
    }

    #[test]
    fn shutdown_awaits_actual_coalesced_save_when_no_barrier_slot_is_available() {
        use std::sync::{Arc, Mutex, mpsc};

        let executor = operations::SerialExecutor::new("github-full-quit-queue-test");
        let (started, running) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let first = executor.submit(move || {
            started.send(())?;
            gate.recv()?;
            Ok(())
        });
        running.recv().unwrap();
        let queued: Vec<_> = (0..7).map(|_| executor.submit(|| Ok(()))).collect();
        let saver = commit_drafts::CoalescingSaver::default();
        let saved = Arc::new(Mutex::new(Vec::<String>::new()));
        let output = saved.clone();
        let capture = |body: &str| {
            let mut fields = entered_fields();
            fields.body = body.into();
            capture_draft(
                "fixture/repository",
                true,
                true,
                ReviewEvent::Comment,
                None,
                fields,
            )
            .unwrap()
        };
        let draft = capture("Before closing");
        let mut completion = None;
        let ui_reply = track_save_completion(
            &mut completion,
            saver.queue_with(&executor, draft.key(), draft, move |batch| {
                output
                    .lock()
                    .unwrap()
                    .extend(batch.values().map(|draft| draft.text().to_owned()));
                Ok(())
            }),
        )
        .unwrap();
        drop(ui_reply);
        assert!(
            futures::executor::block_on(executor.submit(|| Ok(())))
                .unwrap()
                .is_err(),
            "the draft occupied the eighth slot, so an extra barrier must be refused",
        );
        let final_draft = capture("Last characters before closing");
        let response = saver.queue_with(&executor, final_draft.key(), final_draft, |_| {
            panic!("the close edit must join the already accepted save")
        });
        assert!(response.is_none());
        assert!(track_save_completion(&mut completion, response).is_none());
        // The panel and its UI reply can go away; the independent completion
        // still represents the entire coalesced job, without another queue slot.
        drop(saver);
        release.send(()).unwrap();
        assert!(futures::executor::block_on(completion.unwrap()));
        assert_eq!(
            *saved.lock().unwrap(),
            vec!["Last characters before closing"]
        );
        futures::executor::block_on(first).unwrap().unwrap();
        for response in queued {
            futures::executor::block_on(response).unwrap().unwrap();
        }
    }

    #[gpui::test]
    fn window_close_completion_survives_removed_window_until_app_shutdown(cx: &mut TestAppContext) {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };

        // Shutdown excludes tasks from its own foreground session to avoid
        // reentering App. Preference completions come from the background.
        cx.executor().set_block_on_ticks(100..=100);
        let window = cx.add_window(|_, _| gpui::Empty);
        let (complete, response) = futures::channel::oneshot::channel();
        let mut response = Some(response);
        cx.update(|cx| {
            cx.on_window_closed(move |cx, id| {
                if id == window.window_id() {
                    let completion = track_save_completion(&mut None, response.take()).unwrap();
                    retain_window_close_completion(completion, cx);
                }
            })
            .detach();
            window
                .update(cx, |_, window, _| window.remove_window())
                .unwrap();
            assert!(cx.windows().is_empty());
        });
        assert!(
            !complete.is_canceled(),
            "closing the window dropped its final save completion",
        );
        let waited = Arc::new(AtomicBool::new(false));
        let completed = waited.clone();
        cx.executor()
            .spawn(async move {
                complete.send(Ok(())).unwrap();
                completed.store(true, Ordering::SeqCst);
            })
            .detach();
        assert!(!waited.load(Ordering::SeqCst));
        // This executes GPUI's real shutdown observer collection and bounded
        // future wait after the window and its view have already been released.
        cx.quit();
        assert!(waited.load(Ordering::SeqCst));
    }

    #[gpui::test]
    async fn opening_github_from_an_active_repository_update_defers_local_reads(
        cx: &mut TestAppContext,
    ) {
        let (_fixture, app, cx) = repository_window(cx);
        open_panel(&app, cx).await;
        cx.update(|window, cx| {
            assert!(window.has_active_dialog(cx));
            window.close_dialog(cx);
            assert!(!window.has_active_dialog(cx));
        });
    }

    /// A request the panel sends holds the window's operation executor, so
    /// it runs as the window's busy spell, as a write does, labelled with
    /// the panel's busy line: the header's progress says what runs once the
    /// spell looks busy, Changes' Refresh is refused, and a working-state
    /// read asked for meanwhile, as Settings asks on its return, waits for
    /// the request rather than queueing behind it, and runs as it ends. Such
    /// a read once queued behind a review being sent, with nothing saying
    /// why, for as long as GitHub took to answer.
    #[gpui::test]
    async fn a_working_read_waits_for_a_github_request_and_says_why(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, history_script, history_window_from, settle};
        use crate::workspace::{BUSY_LOOK_DELAY, refusals};
        let (fixture, app, cx) = history_window_from(cx, history_script(2, 3)).await;
        // Wide enough that the header draws the busy look's progress label.
        cx.simulate_resize(size(px(1600.), px(900.)));
        let root = fixture.path().join("narrow");
        std::fs::write(root.join("draft.txt"), "draft\n").unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        let destination = "gitturtle-fixture/native-review";
        let panel = cx.update(|window, cx| {
            let repo = app.read(cx).repository.clone().unwrap();
            let panel = cx.new(|cx| Panel::new(app.downgrade(), repo, "main".into(), window, cx));
            panel.update(cx, |panel, cx| {
                panel.review.fixture = true;
                panel.account = Some("offline-reviewer".into());
                panel
                    .destination
                    .update(cx, |input, cx| input.set_value(destination, window, cx));
                panel.close(window, cx);
            });
            app.update(cx, |app, cx| app.open_github(window, cx));
            panel
        });
        settle_panel(&app, cx).await;
        let entries = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| {
                app.work_status.as_ref().map(|status| {
                    status
                        .entries
                        .iter()
                        .map(|entry| entry.path.display().to_string())
                        .collect::<Vec<_>>()
                })
            })
            .unwrap_or_default()
        };
        type Request = fn(&mut Panel, &mut Window, &mut Context<Panel>);
        let requests: [(&str, Request, &str); 2] = [
            (
                "Refresh PRs",
                |panel, window, cx| panel.refresh(1, window, cx),
                RequestKind::Load.label(),
            ),
            (
                "a review comment",
                |panel, window, cx| {
                    let repository = Repository::parse("gitturtle-fixture/native-review").unwrap();
                    panel.confirm = Some(Confirmation {
                        action: Action::Comment {
                            pull: github::review::fixture_pull(false).capture(repository),
                            body: "Looks right.".into(),
                        },
                        account: "offline-reviewer".into(),
                    });
                    panel.submit(window, cx);
                },
                RequestKind::Send.label(),
            ),
        ];
        for (index, (request, send, label)) in requests.into_iter().enumerate() {
            // The request held as a slow network holds it.
            let held = app.update(cx, |app, _| app.operations.hold());
            cx.update(|window, cx| panel.update(cx, |panel, cx| send(panel, window, cx)));
            assert!(
                panel.read_with(cx, |panel, _| panel.pending),
                "{request} is sent"
            );
            let later = format!("later-{index}.txt");
            std::fs::write(root.join(&later), "later\n").unwrap();
            cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
            app.read_with(cx, |app, _| {
                assert!(
                    app.status_task.is_none(),
                    "{request}: no read queues behind it"
                );
                assert_eq!(
                    app.operation_busy,
                    Some(label),
                    "{request}: the window is busy"
                );
                assert!(
                    app.working_read_pending(),
                    "{request}: the read waits for it"
                );
            });
            cx.executor().advance_clock(BUSY_LOOK_DELAY);
            draw(cx);
            assert!(
                cx.debug_bounds("operation-progress").is_some(),
                "{request}: the header says what runs"
            );
            assert_eq!(
                app.read_with(cx, |app, _| app.operation_shown()),
                Some(label)
            );
            assert_eq!(
                refusals::get("refresh-working").map(|refusal| refusal.announced),
                Some(true),
                "{request}: Changes' Refresh is refused"
            );
            drop(held);
            app.read_with(cx, |app, _| app.operations.drain());
            cx.executor().run_until_parked();
            settle(&app, cx).await;
            app.read_with(cx, |app, _| {
                assert_eq!(app.operation_busy, None, "{request}: it ended");
                assert_eq!(app.working_read_deferred, None, "{request}");
            });
            assert!(
                !panel.read_with(cx, |panel, _| panel.pending),
                "{request}: it answered"
            );
            assert!(
                entries(cx).contains(&later),
                "{request}: the read asked for meanwhile ran as it ended"
            );
        }
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
    }

    /// The panel over a window showing History, on the offline fixture,
    /// signed in as its reviewer with its destination named, as
    /// `a_working_read_waits_for_a_github_request_and_says_why` opens it.
    async fn busy_fixture_panel(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        Entity<GitTurtle>,
        Entity<Panel>,
        &mut VisualTestContext,
    ) {
        busy_fixture_panel_from(cx, crate::views::tests::history_script(2, 3)).await
    }

    /// [`busy_fixture_panel`] with a new PR's base, `main`, holding a PR template.
    async fn templated_fixture_panel(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        Entity<GitTurtle>,
        Entity<Panel>,
        &mut VisualTestContext,
    ) {
        const MESSAGE: &str = "Add the PR template";
        const TEMPLATE: &str = "## What changes\n";
        let script = crate::views::tests::history_script(2, 3);
        let script = format!(
            "{}commit refs/heads/main\ncommitter Fixture <fixture@example.invalid> 1700000002 +0000\n\
             data {}\n{MESSAGE}\nM 100644 inline .github/pull_request_template.md\ndata {}\n\
             {TEMPLATE}\ndone\n",
            script.strip_suffix("done\n").expect("the script's end"),
            MESSAGE.len(),
            TEMPLATE.len(),
        );
        busy_fixture_panel_from(cx, script).await
    }

    /// [`busy_fixture_panel`] over the repository `script` imports.
    async fn busy_fixture_panel_from(
        cx: &mut TestAppContext,
        script: String,
    ) -> (
        tempfile::TempDir,
        Entity<GitTurtle>,
        Entity<Panel>,
        &mut VisualTestContext,
    ) {
        let (fixture, app, cx) = crate::views::tests::history_window_from(cx, script).await;
        let panel = cx.update(|window, cx| {
            let repo = app.read(cx).repository.clone().unwrap();
            let panel = cx.new(|cx| Panel::new(app.downgrade(), repo, "main".into(), window, cx));
            panel.update(cx, |panel, cx| {
                panel.review.fixture = true;
                panel.account = Some("offline-reviewer".into());
                panel.destination.update(cx, |input, cx| {
                    input.set_value("gitturtle-fixture/native-review", window, cx)
                });
                panel.close(window, cx);
            });
            app.update(cx, |app, cx| app.open_github(window, cx));
            panel
        });
        settle_panel(&app, cx).await;
        (fixture, app, panel, cx)
    }

    /// A review comment confirmed for sending.
    fn confirmed_comment() -> Confirmation {
        let repository = Repository::parse("gitturtle-fixture/native-review").unwrap();
        Confirmation {
            action: Action::Comment {
                pull: github::review::fixture_pull(false).capture(repository),
                body: "Looks right.".into(),
            },
            account: "offline-reviewer".into(),
        }
    }

    /// Send pressed while a Git write runs is refused: the panel says why
    /// and sends nothing, and the confirmation stays, so Send sends it once
    /// the write has ended. The refusal once took the confirmation with it,
    /// and the submission had to be reviewed again before it could be sent.
    #[gpui::test]
    async fn a_confirmation_a_git_write_refuses_stays_to_send(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, settle};
        let (_fixture, app, panel, cx) = busy_fixture_panel(cx).await;
        // Stage all, held as a slow disk holds it.
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.write(
                    gitturtle_core::WriteCommand::StageAll,
                    "Staging files…",
                    window,
                    cx,
                )
            })
        });
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.confirm = Some(confirmed_comment());
                panel.submit(window, cx);
            })
        });
        draw(cx);
        panel.read_with(cx, |panel, _| {
            assert!(!panel.pending, "nothing is sent");
            assert!(panel.confirm.is_some(), "the confirmation stays");
            assert_eq!(
                panel.error.as_deref(),
                Some("Wait for the current Git operation to finish, then try again.")
            );
        });
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_busy),
            Some("Staging files…"),
            "the write still runs"
        );
        assert!(cx.debug_bounds("github-confirmation").is_some());
        assert!(cx.debug_bounds("github-error").is_some(), "it says why");
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        settle(&app, cx).await;
        // Send again, the write ended: the confirmed comment is sent.
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.submit(window, cx)));
        draw(cx);
        panel.read_with(cx, |panel, _| {
            assert!(panel.pending, "the comment is sent");
            assert!(panel.confirm.is_none(), "its confirmation goes");
            assert_eq!(panel.error, None);
        });
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_busy),
            Some(RequestKind::Send.label())
        );
        assert!(cx.debug_bounds("github-confirmation").is_none());
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        settle_panel(&app, cx).await;
        panel.read_with(cx, |panel, _| {
            assert!(!panel.pending, "it answered");
            assert_eq!(panel.error, None, "the fixture accepted it");
        });
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
    }

    /// Each kind of request the panel sends names itself in the panel's
    /// busy line and the window's operation bar, and what the busy spell
    /// refuses meanwhile gives that kind's reason: Projects' tooltip, and the
    /// palette's and the Linux Main Menu's rows. Connecting the account and
    /// Disconnect once read "Loading your requested PR context…", and the
    /// refusals said to wait for "the current Git operation". A Git write's
    /// refusals still name the Git operation.
    #[gpui::test]
    async fn each_github_request_names_itself_and_what_refusals_wait_for(cx: &mut TestAppContext) {
        use crate::command_palette::CommandId;
        use crate::views::tests::{draw, settle};
        use crate::workspace::{BUSY_LOOK_DELAY, BUSY_REASON, refusals};
        let (_fixture, app, panel, cx) = busy_fixture_panel(cx).await;
        // Projects' tooltip ends with its shortcut, after any reason.
        let projects_key = crate::shortcuts::label(crate::shortcuts::ShortcutId::Projects);
        // Wide enough that the header draws the busy look's progress label,
        // and Changes, whose header has Projects, where History's root has
        // Back to Projects.
        cx.simulate_resize(size(px(1600.), px(900.)));
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        settle(&app, cx).await;
        // Once the spell looks busy: Projects' tooltip and Tags' row.
        let refusals_once_busy = |cx: &mut VisualTestContext| {
            cx.executor().advance_clock(BUSY_LOOK_DELAY);
            draw(cx);
            (
                refusals::tooltip("projects").map(|tooltip| tooltip.to_string()),
                cx.update(|window, cx| {
                    app.read(cx)
                        .menu_command_reason(CommandId::Tags, window, cx)
                }),
            )
        };
        let mut labels = Vec::new();
        for kind in RequestKind::ALL {
            // The request held as a slow network holds it.
            let held = app.update(cx, |app, _| app.operations.hold());
            let sent = cx.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    panel.run(
                        kind,
                        |_| Ok(()),
                        |_: anyhow::Result<()>, _, _, _| {},
                        window,
                        cx,
                    )
                })
            });
            assert!(sent, "{kind:?} is sent");
            let (tooltip, palette) = refusals_once_busy(cx);
            assert_eq!(
                app.read_with(cx, |app, _| app.operation_shown()),
                Some(kind.label()),
                "{kind:?}: the operation bar names it"
            );
            assert!(cx.debug_bounds("operation-progress").is_some());
            assert_eq!(
                panel.read_with(cx, |panel, _| panel.request.label()),
                kind.label(),
                "{kind:?}: so does the panel's busy line"
            );
            assert!(cx.debug_bounds("github-busy").is_some());
            assert_eq!(
                tooltip,
                Some(format!(
                    "Open Projects · {} · {projects_key}",
                    kind.reason()
                )),
                "{kind:?}: Projects waits for it"
            );
            assert_eq!(palette, Some(kind.reason()), "{kind:?}: so does Tags");
            labels.push(kind.label());
            drop(held);
            app.read_with(cx, |app, _| app.operations.drain());
            settle_panel(&app, cx).await;
            assert_eq!(app.read_with(cx, |app, _| app.operation_busy), None);
        }
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(
            labels.len(),
            RequestKind::ALL.len(),
            "each kind has its own line"
        );
        // Stage all, held: its refusals name the Git operation.
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.write(
                    gitturtle_core::WriteCommand::StageAll,
                    "Staging files…",
                    window,
                    cx,
                )
            })
        });
        let (tooltip, palette) = refusals_once_busy(cx);
        assert_eq!(
            tooltip.as_deref(),
            Some(format!("Open Projects · {BUSY_REASON} · {projects_key}").as_str())
        );
        assert_eq!(palette, Some(BUSY_REASON));
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        settle(&app, cx).await;
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
    }

    /// A PR from the window's first commit to its latest, both local, for
    /// its Changes.
    fn local_pull(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> PullRequest {
        let (base, head) = app.read_with(cx, |app, _| {
            (
                app.commits.last().unwrap().oid.clone(),
                app.commits.first().unwrap().oid.clone(),
            )
        });
        assert_ne!(base, head);
        serde_json::from_value(serde_json::json!({
            "number": 7, "title": "Local", "body": null, "state": "open", "draft": false,
            "user": {"login": "river-chen"},
            "head": {"sha": head, "ref": "feature", "label": "river-chen:feature"},
            "base": {"sha": base, "ref": "main", "label": "gitturtle-fixture:main"}
        }))
        .unwrap()
    }

    /// The panel's local work, started from the panel's own callers, names
    /// itself in what the busy spell refuses meanwhile: a new PR's
    /// templates, read from its base in the local repository, say to wait
    /// for the pull request templates, and a PR's Changes, compared there,
    /// for the pull request comparison, in Projects' tooltip and the
    /// palette's and the Linux Main Menu's rows. Both once said to wait for
    /// "the GitHub request", which neither sends, and the templates read
    /// ran as a PR context load, "Loading your requested PR context…".
    #[gpui::test]
    async fn the_panels_local_work_names_itself_in_what_refusals_wait_for(cx: &mut TestAppContext) {
        use crate::command_palette::CommandId;
        use crate::views::tests::{draw, settle};
        use crate::workspace::{BUSY_LOOK_DELAY, refusals};
        type Start = fn(&mut Panel, &mut Window, &mut Context<Panel>);
        let (_fixture, app, panel, cx) = busy_fixture_panel(cx).await;
        // Wide enough that the header draws the busy look's progress label,
        // and Changes, whose header has Projects.
        cx.simulate_resize(size(px(1600.), px(900.)));
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        settle(&app, cx).await;
        let pull = local_pull(&app, cx);
        cx.update(|_, cx| panel.update(cx, |panel, _| panel.pull = Some(pull)));
        // The templates first: the comparison, once it lands, closes the
        // panel to show its changes.
        let work: [(&str, Start, &str, &str); 2] = [
            (
                "Templates",
                Panel::load_templates,
                "Reading the PR templates locally…",
                "Wait for the pull request templates to load",
            ),
            (
                "Changes",
                Panel::changes,
                "Comparing the PR's changes locally…",
                "Wait for the pull request comparison to finish",
            ),
        ];
        for (name, start, label, reason) in work {
            // Held as a large repository holds it.
            let held = app.update(cx, |app, _| app.operations.hold());
            cx.update(|window, cx| panel.update(cx, |panel, cx| start(panel, window, cx)));
            cx.executor().advance_clock(BUSY_LOOK_DELAY);
            draw(cx);
            assert_eq!(
                app.read_with(cx, |app, _| app.operation_shown()),
                Some(label),
                "{name}: the operation bar names it"
            );
            assert_eq!(
                refusals::tooltip("projects").map(|tooltip| tooltip.to_string()),
                Some(format!(
                    "Open Projects · {reason} · {}",
                    crate::shortcuts::label(crate::shortcuts::ShortcutId::Projects)
                )),
                "{name}: Projects waits for it"
            );
            assert_eq!(
                cx.update(|window, cx| {
                    app.read(cx)
                        .menu_command_reason(CommandId::Tags, window, cx)
                }),
                Some(reason),
                "{name}: so does Tags"
            );
            drop(held);
            app.read_with(cx, |app, _| app.operations.drain());
            settle_panel(&app, cx).await;
            assert_eq!(
                app.read_with(cx, |app, _| app.operation_busy),
                None,
                "{name} ended"
            );
        }
    }

    /// Connect GitHub CLI account and Disconnect GitTurtle, pressed in the
    /// panel, each name themselves in the window's operation bar and the
    /// panel's busy line: their own buttons choose the request's kind, which
    /// a test sending each kind through `Panel::run` cannot tell. Both once
    /// read "Loading your requested PR context…". Cancelled from the panel
    /// before they run, neither reaches the account: the account stays and
    /// the panel says it was cancelled, where Disconnect's removal once
    /// ignored Cancel. A test build reaches the account's stand-in
    /// (`account`), which would connect it or empty it, so a guard that
    /// regressed fails here rather than reading a developer's GitHub CLI
    /// token or removing their stored authorization.
    #[gpui::test]
    async fn connect_and_disconnect_name_themselves_from_their_buttons(cx: &mut TestAppContext) {
        use crate::copy_feedback::tests::click;
        use crate::views::tests::draw;
        let (_fixture, app, panel, cx) = busy_fixture_panel(cx).await;
        // Dialogs slide in on the wall clock, which the test clock does not
        // hold; opened at rest, every frame draws the panel in place. The
        // offline fixture leaves Connect and Disconnect unavailable.
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
        cx.update(|_, cx| {
            cx.set_reduce_motion(true);
            panel.update(cx, |panel, _| panel.review.fixture = false);
        });
        open_notice_panel(&app, &panel, (13, 1480., 980.), cx).await;
        click(cx, "github-account-options");
        cx.run_until_parked();
        draw(cx);
        for (button, kind) in [
            ("github-connect", RequestKind::Connect),
            ("github-disconnect", RequestKind::Disconnect),
        ] {
            // Held as a slow GitHub CLI or secure store holds it.
            let held = app.update(cx, |app, _| app.operations.hold());
            click(cx, button);
            draw(cx);
            assert_eq!(
                app.read_with(cx, |app, _| app.operation_busy),
                Some(kind.label()),
                "{button}: the operation bar names it"
            );
            assert_eq!(
                panel.read_with(cx, |panel, _| panel.request.label()),
                kind.label(),
                "{button}: so does the panel's busy line"
            );
            click(cx, "github-cancel");
            drop(held);
            app.read_with(cx, |app, _| app.operations.drain());
            settle_panel(&app, cx).await;
            draw(cx);
            panel.read_with(cx, |panel, _| {
                assert!(!panel.pending, "{button}: it answered");
                assert_eq!(
                    panel.account.as_deref(),
                    Some("offline-reviewer"),
                    "{button}: the account stays"
                );
                assert_eq!(
                    (panel.error.as_deref(), panel.cancelled),
                    (None, kind.cancelled(true)),
                    "{button}: the panel says it was cancelled"
                );
            });
        }
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
    }

    /// Connect stopped by Cancel leaves the account as it was and says so: with an account
    /// connected, "Cancelled · the account is unchanged", where it once said no account was
    /// connected while the toolbar named the connected one; with none, that none was.
    #[gpui::test]
    async fn a_cancelled_connect_says_what_it_left_of_the_account(cx: &mut TestAppContext) {
        use crate::copy_feedback::tests::click;
        use crate::views::tests::draw;
        let (_fixture, app, panel, cx) = busy_fixture_panel(cx).await;
        // Opened at rest, so a click lands where the last frame drew its button. The offline
        // fixture leaves Connect unavailable.
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
        cx.update(|_, cx| {
            cx.set_reduce_motion(true);
            panel.update(cx, |panel, _| panel.review.fixture = false);
        });
        open_notice_panel(&app, &panel, (13, 1480., 980.), cx).await;
        click(cx, "github-account-options");
        cx.run_until_parked();
        draw(cx);
        for (account, says) in [
            (
                Some("offline-reviewer"),
                "Cancelled · the account is unchanged",
            ),
            (None, "Cancelled · no account was connected"),
        ] {
            cx.update(|_, cx| {
                panel.update(cx, |panel, cx| {
                    panel.account = account.map(str::to_owned);
                    cx.notify();
                })
            });
            draw(cx);
            // Held as a slow GitHub CLI holds it, then the panel's Cancel.
            let held = app.update(cx, |app, _| app.operations.hold());
            click(cx, "github-connect");
            draw(cx);
            click(cx, "github-cancel");
            drop(held);
            app.read_with(cx, |app, _| app.operations.drain());
            settle_panel(&app, cx).await;
            draw(cx);
            assert_eq!(
                panel.read_with(cx, |panel, _| (panel.account.clone(), panel.cancelled)),
                (account.map(str::to_owned), Some(says)),
                "{account:?}: the account stays and the panel says so"
            );
            assert!(
                cx.debug_bounds("github-cancelled").is_some(),
                "{account:?}: on the panel's top line"
            );
        }
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
    }

    /// The panel's own Cancel says what it asked for in the request's own words
    /// (`RequestKind::requested`), as the operation bar's Cancel does in its tooltip: it said
    /// "Your review draft is retained" while Connect, Disconnect or a comparison ran, where no
    /// review draft is involved. Connect's success names the platform's store from one place
    /// (`SecureStore::NAME`): it named macOS Keychain on every platform.
    #[gpui::test]
    async fn the_panel_says_what_its_cancel_and_connect_did(cx: &mut TestAppContext) {
        use crate::copy_feedback::tests::click;
        use crate::views::tests::draw;
        let (_fixture, app, panel, cx) = busy_fixture_panel(cx).await;
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
        cx.update(|_, cx| {
            cx.set_reduce_motion(true);
            panel.update(cx, |panel, _| panel.review.fixture = false);
        });
        open_notice_panel(&app, &panel, (13, 1480., 980.), cx).await;
        for kind in RequestKind::ALL {
            let held = app.update(cx, |app, _| app.operations.hold());
            cx.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    panel.run(
                        kind,
                        |_| Ok(()),
                        |_: anyhow::Result<()>, _, _, _| {},
                        window,
                        cx,
                    )
                })
            });
            draw(cx);
            click(cx, "github-cancel");
            let notice = panel.read_with(cx, |panel, _| panel.notice.clone());
            assert_eq!(notice.as_deref(), Some(kind.requested()), "{kind:?}");
            if kind == RequestKind::Connect {
                // What a stopped Connect may have left, in this notice and the operation
                // bar's tooltip: macOS Keychain and the desktop keyring on Linux can hold the
                // authorization.
                for line in [notice.as_deref().unwrap_or_default(), kind.cancel().1] {
                    if cfg!(any(target_os = "macos", target_os = "linux")) {
                        assert!(
                            line.ends_with(
                                "Authorization may already be stored; Disconnect removes it."
                            ),
                            "{line}"
                        );
                    } else {
                        assert!(
                            line.ends_with("GitTurtle stores no authorization on this platform.")
                                && !line.contains("may already be stored"),
                            "nothing can be stored here: {line}"
                        );
                    }
                }
            }
            if matches!(
                kind,
                RequestKind::Connect | RequestKind::Disconnect | RequestKind::Compare
            ) {
                assert!(
                    !kind.requested().contains("review draft"),
                    "{kind:?} involves no review draft"
                );
            }
            drop(held);
            app.read_with(cx, |app, _| app.operations.drain());
            settle_panel(&app, cx).await;
        }

        // Connect, reaching the test build's stand-in for the account.
        click(cx, "github-account-options");
        cx.run_until_parked();
        draw(cx);
        click(cx, "github-connect");
        app.read_with(cx, |app, _| app.operations.drain());
        settle_panel(&app, cx).await;
        let notice = panel.read_with(cx, |panel, _| panel.notice.clone().unwrap_or_default());
        assert_eq!(
            notice,
            format!(
                "Connected as {}; authorization is stored in {}.",
                account::LOGIN,
                SecureStore::NAME
            )
        );
        #[cfg(target_os = "linux")]
        {
            assert_eq!(
                notice,
                format!(
                    "Connected as {}; authorization is stored in your desktop keyring.",
                    account::LOGIN
                )
            );
            assert!(!misnames_the_linux_store(&notice), "{notice}");
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        assert!(
            !notice.contains("Keychain"),
            "no Keychain outside macOS: {notice}"
        );
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
    }

    /// Whether `text` names a store Linux does not have, or says the desktop keyring encrypts
    /// what it holds, which a keyring without a password does not.
    #[cfg(target_os = "linux")]
    fn misnames_the_linux_store(text: &str) -> bool {
        let lower = text.to_lowercase();
        ["keychain", "encrypt", "secure storage"]
            .iter()
            .any(|word| lower.contains(word))
    }

    /// Disconnect reaches the account with its request's own control (`account::remove`), so
    /// the panel's Cancel reaches the keyring's prompt on Linux, where a fresh control would
    /// leave the prompt waiting for its two-minute bound. The stand-in records the control it
    /// got; cancelling the request's control then shows there.
    #[gpui::test]
    async fn disconnect_reaches_the_account_with_its_own_cancel(cx: &mut TestAppContext) {
        use crate::copy_feedback::tests::click;
        use crate::views::tests::draw;
        let (_fixture, app, panel, cx) = busy_fixture_panel(cx).await;
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
        cx.update(|_, cx| {
            cx.set_reduce_motion(true);
            panel.update(cx, |panel, _| panel.review.fixture = false);
        });
        open_notice_panel(&app, &panel, (13, 1480., 980.), cx).await;
        click(cx, "github-account-options");
        cx.run_until_parked();
        draw(cx);
        // Held, so the request's control can be read before the removal runs.
        let held = app.update(cx, |app, _| app.operations.hold());
        click(cx, "github-disconnect");
        draw(cx);
        let control = panel
            .read_with(cx, |panel, _| panel.control.clone())
            .expect("Disconnect runs with its own control");
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        settle_panel(&app, cx).await;
        draw(cx);
        panel.read_with(cx, |panel, _| {
            assert_eq!(
                (panel.account.as_deref(), panel.error.as_deref()),
                (None, None),
                "the stand-in emptied the store"
            );
        });
        let got = account::removed_with();
        assert_eq!(got.len(), 1, "Disconnect reached the stand-in once");
        assert!(
            !got[0].is_cancelled(),
            "nothing cancelled the removal's control"
        );
        control.cancel();
        assert!(
            got[0].is_cancelled(),
            "the removal got the request's own control, which its Cancel reaches"
        );
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
    }

    /// What the panel shows and names for assistive technology through Connect, Disconnect
    /// and a failed stored read (Refresh PRs, which a test build refuses for want of a stored
    /// account, where a locked or missing keyring refuses it with its own line) names the
    /// store from one place and, on Linux, never Keychain, encryption or secure storage:
    /// every label the window wrote, and the names, values and descriptions of the panel's
    /// lines. A test build's panel reaches no token; the keyring's own messages, in the forms
    /// the panel shows them, carry none
    /// (`secret_service::tests::an_accounts_life_in_the_keyring_shows_its_token_nowhere`).
    #[gpui::test]
    async fn the_account_pane_names_only_the_desktop_keyring(cx: &mut TestAppContext) {
        use crate::copy_feedback::tests::click;
        use crate::native_accessibility::written;
        use crate::views::tests::draw;
        let (_fixture, app, panel, cx) = busy_fixture_panel(cx).await;
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
        cx.update(|_, cx| {
            cx.set_reduce_motion(true);
            panel.update(cx, |panel, _| panel.review.fixture = false);
        });
        open_notice_panel(&app, &panel, (13, 1480., 980.), cx).await;
        click(cx, "github-account-options");
        cx.run_until_parked();
        draw(cx);
        let mut texts = Vec::new();
        for step in ["connect", "disconnect", "refresh"] {
            match step {
                "connect" => click(cx, "github-connect"),
                "disconnect" => click(cx, "github-disconnect"),
                _ => cx.update(|window, cx| {
                    panel.update(cx, |panel, cx| panel.refresh(1, window, cx))
                }),
            }
            app.read_with(cx, |app, _| app.operations.drain());
            settle_panel(&app, cx).await;
            written::forget();
            draw(cx);
            draw(cx);
            let (notice, error) =
                panel.read_with(cx, |panel, _| (panel.notice.clone(), panel.error.clone()));
            match step {
                "connect" => assert_eq!(
                    notice,
                    Some(format!(
                        "Connected as {}; authorization is stored in {}.",
                        account::LOGIN,
                        SecureStore::NAME
                    ))
                ),
                "disconnect" => assert!(
                    notice.as_deref().is_some_and(
                        |notice| notice.starts_with("GitTurtle authorization removed.")
                    ),
                    "{notice:?}"
                ),
                _ => assert!(
                    error.as_deref().is_some_and(
                        |error| error.starts_with("A test build has no GitHub account")
                    ),
                    "the stored read failed: {error:?}"
                ),
            }
            texts.extend(notice.into_iter().chain(error));
            for (_, name) in written::labels() {
                texts.extend(name);
            }
            for id in ["github-notice", "github-error", "github-connection-help"] {
                if let Some(line) = written::get(id) {
                    let node = line.node;
                    texts.extend(
                        [node.label(), node.value(), node.description()]
                            .into_iter()
                            .flatten()
                            .map(str::to_owned),
                    );
                }
            }
        }
        assert!(
            texts.iter().any(|text| text.contains(SecureStore::NAME)),
            "the panel names its store: {texts:?}"
        );
        #[cfg(target_os = "linux")]
        for text in &texts {
            assert!(!misnames_the_linux_store(text), "{text}");
        }
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
    }

    /// While a request of the panel runs, the operation bar's Cancel, labelled
    /// or as its icon, is named after what it cancels and its tooltip says
    /// what stops, from the request's kind (`RequestKind::cancel`): it was
    /// named "Cancel the running Git operation" and said it stopped "Git and
    /// its child processes" while the panel connected an account. A Git
    /// write's Cancel still names the Git operation. A new PR's templates,
    /// read in the local repository, stop at the bar's Cancel, as its name
    /// says: none shows and the panel says the read was cancelled, where the
    /// read once ran to its end and showed them. The fixture's base holds a
    /// template, which the read finds uncancelled: with none, a read that
    /// ignored Cancel showed nothing all the same.
    #[gpui::test]
    async fn the_operation_bars_cancel_names_what_it_cancels(cx: &mut TestAppContext) {
        use crate::authentication::{CANCEL_LABELS, CANCEL_NAME, CANCEL_TOOLTIP};
        let (_fixture, app, panel, cx) = templated_fixture_panel(cx).await;
        // Cancel as the window would build it now, labelled or as its icon:
        // its accessible name and its tooltip.
        let cancel = |icon: bool, cx: &mut VisualTestContext| {
            let owner = app.clone();
            let node = crate::views::tests::view_node(cx, move |window, cx| {
                use gpui::Element as _;
                let button = owner
                    .update(cx, |app, cx| app.operation_cancel(icon, cx))
                    .expect("Cancel")
                    .base(window, cx);
                let element = RenderOnce::render(button, window, cx).into_element();
                let mut node =
                    gpui::accesskit::Node::new(element.a11y_role().expect("Cancel's role"));
                element.write_a11y_info(&mut node);
                node
            });
            let tooltip = crate::views::CONTROL_TOOLTIPS
                .with(|tooltips| tooltips.borrow().get("cancel-git-operation").cloned());
            (node.label().map(str::to_owned), tooltip)
        };
        let says = |name: &str, stops: &str, icon: bool| {
            let tooltip = if icon {
                format!("{} · {stops}", CANCEL_LABELS[0])
            } else {
                stops.to_owned()
            };
            (Some(name.to_owned()), Some(tooltip))
        };
        for kind in RequestKind::ALL {
            // The request held as a slow network holds it.
            let held = app.update(cx, |app, _| app.operations.hold());
            let sent = cx.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    panel.run(
                        kind,
                        |_| Ok(()),
                        |_: anyhow::Result<()>, _, _, _| {},
                        window,
                        cx,
                    )
                })
            });
            assert!(sent, "{kind:?} is sent");
            let (name, stops) = kind.cancel();
            for icon in [false, true] {
                assert_eq!(
                    cancel(icon, cx),
                    says(name, stops, icon),
                    "{kind:?}: Cancel{} names what it cancels",
                    if icon { " as its icon" } else { "" }
                );
            }
            drop(held);
            app.read_with(cx, |app, _| app.operations.drain());
            settle_panel(&app, cx).await;
        }
        // A Git write's Cancel names the Git operation.
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.write(
                    gitturtle_core::WriteCommand::StageAll,
                    "Staging files…",
                    window,
                    cx,
                )
            })
        });
        for icon in [false, true] {
            assert_eq!(
                cancel(icon, cx),
                says(CANCEL_NAME, CANCEL_TOOLTIP, icon),
                "a Git write"
            );
        }
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;

        // The templates read finds the base's template.
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.load_templates(window, cx)));
        app.read_with(cx, |app, _| app.operations.drain());
        settle_panel(&app, cx).await;
        assert_eq!(
            panel.read_with(cx, |panel, _| panel
                .templates
                .iter()
                .map(|template| template.path.clone())
                .collect::<Vec<_>>()),
            [".github/pull_request_template.md"],
            "the base holds a template"
        );
        cx.update(|_, cx| panel.update(cx, |panel, _| panel.templates.clear()));

        // The templates read, held, then the bar's Cancel.
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.load_templates(window, cx)));
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_busy),
            Some(RequestKind::Templates.label()),
            "the templates read runs as the busy spell"
        );
        cx.update(|window, cx| app.update(cx, |app, cx| app.cancel_operation(window, cx)));
        assert!(
            panel.read_with(cx, |panel, _| panel
                .local_control
                .as_ref()
                .is_some_and(HistoryCancellation::is_cancelled)),
            "the bar's Cancel stops the templates read"
        );
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        settle_panel(&app, cx).await;
        assert_eq!(app.read_with(cx, |app, _| app.operation_busy), None);
        panel.read_with(cx, |panel, _| {
            assert!(panel.templates.is_empty(), "no template shows");
            assert_eq!(
                (panel.error.as_deref(), panel.cancelled),
                (None, RequestKind::Templates.cancelled(false)),
                "the panel says the read was cancelled"
            );
        });
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
    }

    /// The operation bar's Cancel (`GitTurtle::cancel_operation`) stops a
    /// PR's Changes, as the panel's own Cancel does: the comparison reads
    /// through its own cancellation, which the bar's Cancel once never
    /// reached, so the comparison ran on and opened.
    #[gpui::test]
    async fn the_operation_bars_cancel_stops_a_prs_changes(cx: &mut TestAppContext) {
        let (_fixture, app, panel, cx) = busy_fixture_panel(cx).await;
        let pull = local_pull(&app, cx);
        cx.update(|_, cx| panel.update(cx, |panel, _| panel.pull = Some(pull)));
        // Changes, held as a large comparison holds it, then the bar's Cancel.
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.changes(window, cx)));
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_busy),
            Some(RequestKind::Compare.label()),
            "Changes runs as the busy spell"
        );
        cx.update(|window, cx| app.update(cx, |app, cx| app.cancel_operation(window, cx)));
        assert!(
            panel.read_with(cx, |panel, _| panel
                .local_control
                .as_ref()
                .is_some_and(HistoryCancellation::is_cancelled)),
            "the bar's Cancel stops the comparison"
        );
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        settle_panel(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert_eq!(app.operation_busy, None, "Changes ended");
            assert!(!app.revision_inspection.is_active(), "no comparison opens");
        });
        assert_eq!(
            panel.read_with(cx, |panel, _| (panel.error.clone(), panel.cancelled)),
            (None, RequestKind::Compare.cancelled(false)),
            "the panel says it did not compare, as cancelled"
        );
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
    }

    /// A PR's Changes stopped by Cancel, the panel's own or the operation
    /// bar's, says so in the failure's place: "Cancelled", muted, a status
    /// the window says politely rather than as an alert, since the person
    /// asked for it, until the next request clears it as it clears a
    /// failure. It once showed the failure telling the person to Fetch. A
    /// failure the comparison met before the cancel reached it, as a
    /// captured revision Git cannot take, still shows its error there.
    #[gpui::test]
    async fn a_cancelled_prs_changes_say_they_were_cancelled(cx: &mut TestAppContext) {
        use crate::copy_feedback::tests::{alerted, announced, click};
        use crate::native_accessibility::written;
        use crate::views::tests::draw;
        let (_fixture, app, panel, cx) = busy_fixture_panel(cx).await;
        // Opened at rest, so a click lands where the last frame drew its button.
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
        let pull = local_pull(&app, cx);
        cx.update(|_, cx| {
            cx.set_reduce_motion(true);
            panel.update(cx, |panel, _| panel.pull = Some(pull.clone()));
        });
        open_notice_panel(&app, &panel, (13, 1480., 980.), cx).await;
        let line = RequestKind::Compare
            .cancelled(false)
            .expect("Changes says it was cancelled");

        // Changes, held as a large comparison holds it, then the panel's Cancel.
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.changes(window, cx)));
        draw(cx);
        click(cx, "github-cancel");
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        settle_panel(&app, cx).await;
        draw(cx);
        draw(cx);
        assert!(
            !app.read_with(cx, |app, _| app.revision_inspection.is_active()),
            "no comparison opens"
        );
        assert_eq!(
            panel.read_with(cx, |panel, _| (panel.error.clone(), panel.cancelled)),
            (None, Some(line)),
            "it says it was cancelled, not that it failed"
        );
        assert!(cx.debug_bounds("github-error").is_none(), "no failure line");
        let cancelled = cx
            .debug_bounds("github-cancelled")
            .expect("the cancel's line");
        assert_eq!(
            written::get("github-cancelled").map(|line| line.node.role()),
            Some(Role::Status),
            "a status"
        );
        assert_eq!(
            announced(cx).map(|(_, text)| text).as_deref(),
            Some(line),
            "said politely"
        );
        assert_eq!(alerted(cx), None, "not as an alert");

        // A captured base Git cannot take fails before the bar's Cancel reaches the
        // comparison. Sending it clears the cancel's line, as it clears a failure.
        let mut broken = pull;
        broken.base.sha = String::new();
        cx.update(|_, cx| panel.update(cx, |panel, _| panel.pull = Some(broken)));
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.changes(window, cx)));
        draw(cx);
        assert!(
            cx.debug_bounds("github-cancelled").is_none(),
            "the next request clears the cancel's line"
        );
        cx.update(|window, cx| app.update(cx, |app, cx| app.cancel_operation(window, cx)));
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        settle_panel(&app, cx).await;
        draw(cx);
        draw(cx);
        assert_eq!(
            panel.read_with(cx, |panel, _| panel.cancelled),
            None,
            "a failure is no cancel"
        );
        let failure = cx
            .debug_bounds("github-error")
            .expect("the failure it met shows");
        assert_eq!(
            failure.top(),
            cancelled.top(),
            "the cancel's line stood in the failure's place"
        );
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
    }

    /// The sign-in command's copy button, drawn by the panel's own view in
    /// the dialog layer, reads Copied in its own bounds and restores: a copy
    /// redraws the whole window, not only the view that owns its button.
    #[gpui::test]
    async fn the_sign_in_command_reads_copied_inside_the_panel(cx: &mut TestAppContext) {
        use crate::copy_feedback::{RESTORE_AFTER, tests::*};
        let (_fixture, app, cx) = repository_window(cx);
        // Dialogs slide in on the wall clock, which the test clock does not
        // hold: a loaded host let the panel move between the frame that
        // measured a button and the frame its press drew, and the click
        // missed. They open at rest, where every frame draws them in place.
        cx.update(|_, cx| cx.set_reduce_motion(true));
        open_panel(&app, cx).await;
        crate::commit_message::tests::draw(cx);
        click(cx, "github-account-options");
        cx.run_until_parked();
        let bounds = |cx: &mut VisualTestContext| cx.debug_bounds("github-sign-in-help").unwrap();
        let rest = bounds(cx);
        assert_eq!(shows(cx, "github-sign-in-help"), Shows::Rest);

        click(cx, "github-sign-in-help");
        assert_eq!(
            clipboard(cx),
            "gh auth login --hostname github.com --web --skip-ssh-key"
        );
        assert_eq!(shows(cx, "github-sign-in-help"), Shows::Copied);
        assert_eq!(bounds(cx), rest);
        assert_eq!(announced(cx).unwrap().1, "Copied sign-in command");

        wait(cx, RESTORE_AFTER);
        assert_eq!(shows(cx, "github-sign-in-help"), Shows::Rest);
        assert_eq!(bounds(cx), rest);
        assert_eq!(announced(cx), None);
        cx.update(|window, cx| window.close_dialog(cx));
    }

    /// The panel's body takes the height its content needs up to the room the dialog's height
    /// cap leaves it ([`appearance::dialog_max_height`], less the dialog's own chrome), so the
    /// connection pane's help reads whole at 18 pt in a 461 × 490 window: with the content
    /// running past the body, the dialog's foot reaches that cap and stays inside it, the help
    /// shows whole once the body scrolls to it, and Tab onto Connect below it reveals Connect
    /// whole. The body was the window less a fixed 220 px
    /// scaled with the text, 185 px there where the cap leaves 265 px, and native QA saw the
    /// help taller than the body (QA-97). At 13 pt the same holds in that window, and in a
    /// 1,480 × 973 window the content fits: the body holds it unscrolled, the help whole, as
    /// before.
    #[gpui::test]
    async fn the_body_takes_the_room_the_dialog_cap_leaves_and_shows_the_connection_help_whole(
        cx: &mut TestAppContext,
    ) {
        use crate::tags::tests::draw;
        let (_fixture, app, cx) = repository_window(cx);
        // Tab's bindings, the installed focus ring and motion at rest.
        crate::focus_ring::sweep::prepare(&app, cx);
        let panel = cx.update(|window, cx| {
            let repo = app.read(cx).repository.clone().unwrap();
            let panel = cx.new(|cx| Panel::new(app.downgrade(), repo, "main".into(), window, cx));
            panel.update(cx, |panel, cx| panel.close(window, cx));
            panel
        });
        let scroll = panel.read_with(cx, |panel, _| panel.review.body_scroll.clone());
        for (interface, width, height) in [(18, 461., 490.), (13, 461., 490.), (13, 1480., 973.)] {
            let at = format!("{interface} pt in {width} × {height}");
            cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            open_panel(&app, cx).await;
            if !panel.read_with(cx, |panel, _| panel.review.connection_open) {
                crate::copy_feedback::tests::click(cx, "github-account-options");
                cx.run_until_parked();
            }
            scroll.set_offset(point(px(0.), px(0.)));
            draw(cx);
            draw(cx);
            let body = cx.debug_bounds("github-panel").expect("the body");
            let (dialog, foot) = dialog_frame(cx);
            let cap = cx.update(|window, _| appearance::dialog_max_height(window));
            assert!(
                dialog.bottom() <= foot + px(0.5),
                "{at}: the dialog ends at {:?}, below its cap's foot {foot:?} ({cap:?} tall)",
                dialog.bottom()
            );
            let help = cx
                .debug_bounds("github-connection-help")
                .expect("the connection pane's help");
            let inside = |line: Bounds<Pixels>, body: Bounds<Pixels>| {
                line.top() >= body.top() - px(0.5) && line.bottom() <= body.bottom() + px(0.5)
            };
            if width > 1000. {
                assert_eq!(
                    scroll.max_offset().y,
                    px(0.),
                    "{at}: the content fits the body"
                );
                assert!(
                    inside(help, body),
                    "{at}: the help {help:?} shows whole in the body {body:?}"
                );
                cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
                settle_panel(&app, cx).await;
                continue;
            }
            // The content runs past the body, which takes all the room the cap leaves it.
            assert!(
                scroll.max_offset().y > px(0.),
                "{at}: the content runs past the body"
            );
            assert!(
                dialog.bottom() >= foot - px(1.),
                "{at}: the body, {:?} tall, stops short of the room the dialog's cap leaves it: \
                 the dialog ends at {:?}, its cap's foot at {foot:?}",
                body.size.height,
                dialog.bottom()
            );
            // Scrolled to the help, the body shows it whole.
            let offset = (scroll.offset().y + (body.top() - help.top()))
                .clamp(-scroll.max_offset().y, px(0.));
            scroll.set_offset(point(px(0.), offset));
            draw(cx);
            draw(cx);
            let help = cx.debug_bounds("github-connection-help").unwrap();
            assert!(
                inside(help, body),
                "{at}: scrolled to the help, the body {body:?} shows it {help:?} whole"
            );
            // Tab onto Connect, from the body's top, reveals it whole.
            scroll.set_offset(point(px(0.), px(0.)));
            draw(cx);
            let mut stops = 0;
            while !ringed(cx, "github-connect") {
                stops += 1;
                assert!(stops <= 12, "{at}: Tab reaches Connect");
                cx.simulate_keystrokes("tab");
                for _ in 0..3 {
                    crate::width_walk::frame(cx);
                }
            }
            let connect = cx.debug_bounds("github-connect").unwrap();
            assert!(
                inside(connect, body),
                "{at}: Tab shows Connect {connect:?} whole in the body {body:?}"
            );
            cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
            settle_panel(&app, cx).await;
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// How far apart the panel's 12 pt text ([`label`]) draws its lines: GPUI's default line
    /// height, the golden ratio of the text size, snapped to a device pixel, as the text
    /// element lays its lines out. Not [`Window::line_height`], the root style's, which is the
    /// interface size's and rounds to a whole point.
    fn label_line(cx: &mut VisualTestContext) -> Pixels {
        cx.update(|window, _| {
            let size = appearance::ui_text(12.);
            let line = gpui_kit::phi().to_pixels(size.into(), window.rem_size());
            window.pixel_snap(line)
        })
    }

    /// The body and the lines drawn in it, each named, with where it lies, after a fresh draw:
    /// each line of every text inside the body, read from the accessibility nodes the frame
    /// wrote, with its lines at the panel's 12 pt line height (a text of another height counts
    /// as one line), and each control of one line in `controls`.
    fn body_lines(
        cx: &mut VisualTestContext,
        controls: &[&'static str],
    ) -> (Bounds<Pixels>, Vec<(String, Pixels, Pixels)>) {
        use crate::native_accessibility::written;
        written::forget();
        crate::tags::tests::draw(cx);
        let body = cx.debug_bounds("github-panel").expect("the body");
        let line = label_line(cx);
        let scale = cx.update(|window, _| window.scale_factor());
        let mut lines = Vec::new();
        for id in written::inside("github-panel") {
            let Some(text) = written::get(&id) else {
                continue;
            };
            let (Role::Label, Some(layout)) = (text.node.role(), text.layout) else {
                continue;
            };
            let (top, bottom) = (px(layout.y0 as f32 / scale), px(layout.y1 as f32 / scale));
            let count = (bottom - top) / line;
            if (count - count.round()).abs() < 0.05 && count.round() >= 1. {
                for index in 0..count.round() as usize {
                    let at = top + line * index as f32;
                    lines.push((format!("{id} line {}", index + 1), at, at + line));
                }
            } else {
                lines.push((id, top, bottom));
            }
        }
        for control in controls {
            if let Some(frame) = cx.debug_bounds(control) {
                lines.push((control.to_string(), frame.top(), frame.bottom()));
            }
        }
        assert!(
            lines
                .iter()
                .any(|(name, ..)| name.starts_with("github-connection-help")),
            "the body draws the connection help"
        );
        (body, lines)
    }

    /// Presses `keys` until `control` draws its focus ring, drawing three frames after each
    /// press, and returns the body's scroll offset before the last press.
    fn tab_onto(
        cx: &mut VisualTestContext,
        scroll: &ScrollHandle,
        keys: &str,
        control: &'static str,
    ) -> Pixels {
        let mut before = scroll.offset().y;
        let mut stops = 0;
        while !ringed(cx, control) {
            stops += 1;
            assert!(stops <= 12, "{keys} reaches {control}");
            before = scroll.offset().y;
            cx.simulate_keystrokes(keys);
            for _ in 0..3 {
                crate::width_walk::frame(cx);
            }
        }
        before
    }

    /// Holds the body to the rule of its reveals once `keys` moved focus onto `control` from
    /// the body scrolled to `before` ([`review::clear_of_lines`]), and returns the lines cut
    /// at the body's edges, whether it moved or not: the control shows whole with its ring's
    /// room, and where the body moved, neither of its edges falls inside a line (the lines of
    /// [`body_lines`]; half a pixel either way is layout's rounding), or no place the body's
    /// top edge can take keeps the control whole with neither edge inside a line, so the
    /// control whole won. Those places are every one the frame drawn offers, not the reveal's
    /// own: each line's top and foot (the top edge between two lines), each line's top and
    /// foot less the body's height (the bottom edge between two lines), each line's top less
    /// the ring's room (where a block a line starts stands with its ring above it) and both
    /// ends of the content, where the content lets the body scroll. Where the body did not
    /// move, the control was whole there with its ring, which moves nothing. The lines and the
    /// places are read from the frame drawn, so the rule holds however the platform's text
    /// wraps. The places once were the reveal's own, the lines' tops and feet and the
    /// content's ends: at 17 pt in a 440 × 490 window Shift+Tab onto Account options left the
    /// help's fifth line cut at the body's foot, where the bottom edge at that line's foot
    /// clears every line.
    fn assert_reveal_rule(
        cx: &mut VisualTestContext,
        scroll: &ScrollHandle,
        before: Pixels,
        (keys, control): (&str, &'static str),
        controls: &[&'static str],
        at: &str,
    ) -> Vec<String> {
        let (body, lines) = body_lines(cx, controls);
        let ring = cx.update(|_, cx| appearance::button_ring_room(cx));
        let shown = cx.debug_bounds(control).expect(control);
        let slack = px(0.5);
        let (top, bottom) = (shown.top() - ring, shown.bottom() + ring);
        assert!(
            top >= body.top() - slack && bottom <= body.bottom() + slack,
            "{at}: {keys} shows {control} {shown:?} whole with its ring in the body {body:?}"
        );
        let inside = |edge: Pixels, within: Pixels| {
            lines
                .iter()
                .filter(move |(_, top, bottom)| *top < edge - within && *bottom > edge + within)
        };
        let cut: Vec<_> = inside(body.top(), slack)
            .chain(inside(body.bottom(), slack))
            .map(|(name, top, bottom)| format!("{name} {top:?}..{bottom:?}"))
            .collect();
        let offset = scroll.offset().y;
        if cut.is_empty() || offset == before {
            return cut;
        }
        // Where the body's top edge can stop, in this frame's coordinates, with the control
        // whole and no edge inside a line by more than the reveal's own hundredth of a pixel.
        let height = body.size.height;
        let first = body.top() + offset;
        let last = first + scroll.max_offset().y;
        let within = px(0.01);
        let stops: Vec<Pixels> = lines
            .iter()
            .flat_map(|(_, top, bottom)| {
                [*top, *bottom, *top - height, *bottom - height, *top - ring]
            })
            .chain([first, last])
            .filter(|edge| {
                *edge >= first - slack
                    && *edge <= last + slack
                    && *edge <= top + slack
                    && *edge + height >= bottom - slack
                    && inside(*edge, within).next().is_none()
                    && inside(*edge + height, within).next().is_none()
            })
            .collect();
        assert_eq!(
            stops,
            Vec::<Pixels>::new(),
            "{at}: after {keys} onto {control}, {cut:?} cut at the edges of the body {body:?} \
             where its top edge could stop clear of every line"
        );
        cut
    }

    /// A list inside the body clips its rows to its own viewport, so the lines of the rows
    /// it has scrolled out of itself are no lines of the body, which its reveals stop clear of:
    /// only lines a person can see count. The previous attempts' list, at the content's end
    /// and scrolled to its own top, its later rows below its foot, here runs past the body's
    /// bottom edge, where the list's clip and the body's coincide, and the body once kept the
    /// hidden rows' lines all the same (FU15-D's review): phantom lines that could leave a
    /// reveal no place to stop, so it stayed where it cut a line the person sees.
    #[gpui::test]
    async fn lines_a_list_in_the_body_hides_are_no_lines_of_the_body(cx: &mut TestAppContext) {
        use crate::tags::tests::draw;
        let (_fixture, app, cx) = repository_window(cx);
        let panel = cx.update(|window, cx| {
            let repo = app.read(cx).repository.clone().unwrap();
            let panel = cx.new(|cx| Panel::new(app.downgrade(), repo, "main".into(), window, cx));
            panel.update(cx, |panel, cx| panel.close(window, cx));
            panel
        });
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        open_panel(&app, cx).await;
        panel.update(cx, |panel, cx| {
            panel.attempts = (0..10)
                .map(|index| drafts::Attempt {
                    destination: format!("GitHub fixture attempt {index}"),
                    outcome: "Previous outbound result".into(),
                    completed_reply_sha256: None,
                })
                .collect();
            panel.review.recovery_open = true;
            cx.notify();
        });
        draw(cx);
        let scroll = panel.read_with(cx, |panel, _| panel.review.body_scroll.clone());
        // The list's top 40 px above the body's foot, the rest of it below.
        let body = cx.debug_bounds("github-panel").expect("the body");
        let list = cx.debug_bounds("github-attempt-list").expect("the list");
        let offset = scroll.offset().y + (body.bottom() - px(40.) - list.top());
        scroll.set_offset(point(px(0.), offset));
        draw(cx);
        draw(cx);
        let body = cx.debug_bounds("github-panel").unwrap();
        let list = cx.debug_bounds("github-attempt-list").unwrap();
        assert!(
            list.top() < body.bottom() && list.bottom() > body.bottom(),
            "the list {list:?} runs past the body's foot {body:?}"
        );
        let last = cx
            .debug_bounds("github-attempt-9")
            .expect("the last attempt, drawn");
        assert!(
            last.top() > list.bottom(),
            "the list {list:?} hides its last row {last:?} below its foot"
        );
        let lines = panel.read_with(cx, |panel, cx| panel.review.body_blocks.lines(cx));
        let hidden: Vec<_> = lines
            .iter()
            .filter(|(top, bottom)| *bottom > list.bottom() + px(0.5) && *top < last.bottom())
            .collect();
        assert_eq!(
            hidden,
            Vec::<&(Pixels, Pixels)>::new(),
            "no line below the list's foot {list:?} is the body's"
        );
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
    }

    /// The conversation list clips its conversations to its own viewport as the attempts'
    /// list does: scrolled to its top on a PR's Overview at 13 pt in a 461 × 490 window and
    /// running past the body's foot, the lines of its conversations it hides below its own
    /// foot are no lines of the body, where they once counted (FU15-D's review).
    #[gpui::test]
    async fn conversations_the_list_hides_are_no_lines_of_the_body(cx: &mut TestAppContext) {
        let (_fixture, _app, panel, pull, cx) = fixture_panel(cx).await;
        let (body_scroll, list_scroll) = cx.read(|cx| {
            let panel = panel.read(cx);
            (
                panel.review.body_scroll.clone(),
                panel.conversations.list_scroll.clone(),
            )
        });
        show_overview(&panel, &pull, (13, 461., 490.), cx);
        let frames = |cx: &mut VisualTestContext| {
            for _ in 0..3 {
                crate::width_walk::frame(cx);
            }
        };
        // The list's top 40 px above the body's foot, the rest of it below.
        list_scroll.set_offset(point(px(0.), px(0.)));
        let body = cx.debug_bounds("github-panel").expect("the body");
        let list = cx.debug_bounds("github-threads").expect("the list");
        let offset = (body_scroll.offset().y + (body.bottom() - px(40.) - list.top()))
            .clamp(-body_scroll.max_offset().y, px(0.));
        body_scroll.set_offset(point(px(0.), offset));
        frames(cx);
        let body = cx.debug_bounds("github-panel").unwrap();
        let list = cx.debug_bounds("github-threads").unwrap();
        assert!(
            list.top() < body.bottom() && list.bottom() > body.bottom(),
            "the list {list:?} runs past the body's foot {body:?}"
        );
        // Where each text the list hides below its own foot starts, read from the
        // accessibility nodes a fresh frame writes.
        use crate::native_accessibility::written;
        written::forget();
        crate::tags::tests::draw(cx);
        let scale = cx.update(|window, _| window.scale_factor());
        let mut hidden = Vec::new();
        for id in written::inside("github-threads") {
            let Some(text) = written::get(&id) else {
                continue;
            };
            let (Role::Label, Some(layout)) = (text.node.role(), text.layout) else {
                continue;
            };
            // A text's box can stand a little taller than the lines it paints, so only its
            // first line's top is certain: where the text is wholly below the list's foot.
            let top = px(layout.y0 as f32 / scale);
            if top >= list.bottom() - px(0.5) {
                hidden.push(top);
            }
        }
        assert!(
            !hidden.is_empty(),
            "the list {list:?} hides text below its foot"
        );
        let lines = panel.read_with(cx, |panel, cx| panel.review.body_blocks.lines(cx));
        let kept: Vec<_> = lines
            .iter()
            .filter(|(top, _)| {
                hidden
                    .iter()
                    .any(|hidden| (*top - *hidden).abs() < px(0.01))
            })
            .collect();
        assert_eq!(
            kept,
            Vec::<&(Pixels, Pixels)>::new(),
            "no line the list hides below its foot {list:?} is the body's"
        );
    }

    /// Tab onto Connect, enabled in a panel that is no offline fixture and runs no request,
    /// as opening it over a repository leaves it (it reads local settings and drafts only),
    /// shows Connect whole and leaves no line cut at the body's edges at 18 pt and at 13 pt
    /// in a 461 × 490 window: each line of the body's text and each of its one-line controls
    /// lies wholly inside the body or wholly outside it, where the reveal stopped a little
    /// short or went a little further. Tab on onto Copy sign-in command and Disconnect, and
    /// Shift+Tab back up onto Account options, do the same, unless no place the body can
    /// scroll to clears every line with the control whole ([`assert_reveal_rule`]). Revealed
    /// as one part, the connection pane, taller than the body at 18 pt, showed its foot and
    /// left the help's second line cut at the body's top edge (FU14-E's review).
    #[gpui::test]
    async fn a_reveal_leaves_no_line_cut_at_the_body_edges(cx: &mut TestAppContext) {
        walk_connection_reveals(cx, &[18, 13], [461.]).await;
    }

    /// The reveals of [`a_reveal_leaves_no_line_cut_at_the_body_edges`] keep their rule
    /// wherever the connection pane's text wraps, at 18, 17 and 13 pt (the largest interface
    /// size is 18) in windows from 430 px to 500 px wide, 490 px tall, as each platform's text
    /// measures it, every 5 px: on Linux the walk's stops change at 430, 439, 454, 468 and 497
    /// px at 18 pt, 430, 435, 448 and 475 px at 17 pt and 478 px at 13 pt, and each width
    /// costs a quarter of a second. From 468 px at 18 pt the help takes five lines, as macOS
    /// lays it out at 461 px, and no block start kept the control whole with both edges
    /// clear: Tab onto Connect left Copy sign-in command cut at the body's foot, and Tab onto
    /// Copy left Disconnect cut there, 336..375 in the body 93..358, as on macOS (hosted run
    /// 42032620931), where the top edge at the foot of the help's first line clears both.
    /// Shift+Tab back onto Account options there, with the toolbar's controls each on a row
    /// of their own above the pane, has nowhere clear to stop.
    #[gpui::test]
    async fn reveals_keep_their_rule_wherever_the_connection_help_wraps(cx: &mut TestAppContext) {
        walk_connection_reveals(cx, &[18, 17, 13], (430..=500).step_by(5).map(|w| w as f32)).await;
    }

    /// At 17 pt in a 440 × 490 window, as Linux wraps the connection help in six lines,
    /// Shift+Tab from Connect back onto Account options leaves no line cut at the body's
    /// edges: no block start keeps Account options whole with both edges clear, and the body
    /// stops with its bottom edge at the foot of the help's fifth line, 365, its top edge at
    /// 94, between Refresh and New pull request. It once stopped at 91.5, Refresh's foot, and
    /// left that line cut by 2.5 px at the body's foot (FU16-B's review): the body counted
    /// the help's lines 25 px apart, GPUI's line height rounded to a whole point, where the
    /// text draws them 25.5 px apart, its line height snapped to a device pixel, and it took
    /// no place where its bottom edge lies at a line's foot. macOS measures the help in
    /// another font, where the swept test holds the rule.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn shift_tab_onto_account_options_at_17_pt_leaves_the_help_whole(
        cx: &mut TestAppContext,
    ) {
        let walked = walk_connection_reveals(cx, &[17], [440.]).await;
        let step = walked
            .iter()
            .find(|step| step.step == ("shift-tab", "github-account-options"))
            .expect("the walk takes Shift+Tab onto Account options");
        assert_eq!(
            step.cut,
            Vec::<String>::new(),
            "{}: Shift+Tab onto Account options leaves no line cut at the body's edges",
            step.at
        );
    }

    /// A step of [`walk_connection_reveals`]: where it was taken, its keys and the control
    /// they focused, the lines it left cut at the body's edges, the lines it showed whole and
    /// how many lines the connection help took.
    struct Walked {
        at: String,
        #[cfg_attr(
            not(target_os = "linux"),
            allow(dead_code, reason = "only the Linux-only Shift+Tab test reads it")
        )]
        step: (&'static str, &'static str),
        #[cfg_attr(
            not(target_os = "linux"),
            allow(dead_code, reason = "only the Linux-only Shift+Tab test reads it")
        )]
        cut: Vec<String>,
        whole: Vec<String>,
        help: usize,
    }

    /// The names of the lines of [`body_lines`] the body shows whole, half a pixel either way
    /// being layout's rounding, and how many lines the connection help takes.
    fn whole_lines(cx: &mut VisualTestContext, controls: &[&'static str]) -> (Vec<String>, usize) {
        let (body, lines) = body_lines(cx, controls);
        let slack = px(0.5);
        let help = lines
            .iter()
            .filter(|(name, ..)| name.starts_with("github-connection-help line"))
            .count();
        let whole = lines
            .into_iter()
            .filter(|(_, top, bottom)| {
                *top >= body.top() - slack && *bottom <= body.bottom() + slack
            })
            .map(|(name, ..)| name)
            .collect();
        (whole, help)
    }

    /// Tab and Shift+Tab through the connection pane show each line of its help whole at some
    /// stop, at 17 pt in a 440 × 490 window and at 18 pt in a 480 × 490 one, as Linux wraps the
    /// help in six and five lines: each reveal moves the body the least that shows the control
    /// whole with no line cut at its edges ([`review::clear_of_lines`]). Safe stops first, Tab
    /// onto Connect at 17 pt took the body to Connect's row, 386.5 px down, past the help's
    /// second to sixth lines, which no stop of the walk then showed whole, and at 18 pt in 480
    /// px the help's first line was never whole (FU18-D's review).
    #[gpui::test]
    async fn the_connection_walk_shows_every_help_line_whole(cx: &mut TestAppContext) {
        let walked = walk_connection_reveals(cx, &[17, 18], [440., 480.]).await;
        assert_every_help_line_whole(&walked);
    }

    /// With a repository path of about 120 characters ([`long_path_window`]), at 13, 17 and 18
    /// pt in a 440 × 490 window, the connection pane's "Local worktree:" line keeps to one
    /// line, the path elided in its middle keeping its top directory and its last component
    /// ("Local worktree: /tmp/…/compact" where that fits beside the label), measured as laid
    /// out and fitting the line, so the line's own elision cuts nothing more, with the whole
    /// path its accessible name and its tooltip; and each line of the help is whole at some
    /// stop of the pane's Tab walk. The line wrapped with the path: six lines at 17 pt, and
    /// then no stop of the walk showed the help's first line whole (FU26-A's review).
    #[gpui::test]
    async fn a_long_worktree_path_keeps_to_one_line_and_leaves_the_help_whole(
        cx: &mut TestAppContext,
    ) {
        use crate::native_accessibility::written;
        // A GPUI test's seeds share the thread and the tooltips noted on it.
        crate::views::CONTROL_TOOLTIPS.with(|tooltips| tooltips.borrow_mut().clear());
        let mut checked = 0;
        let check = |cx: &mut VisualTestContext, at: &str, path: &std::path::Path| {
            let whole = path.display().to_string();
            assert!(
                whole.len() >= 110,
                "{at}: the fixture's path is long: {whole}"
            );
            let top = format!("/{}", whole[1..].split('/').next().unwrap());
            let line = cx
                .debug_bounds("github-worktree")
                .expect("the worktree line");
            let height = label_line(cx);
            assert!(
                line.size.height <= height + px(0.5),
                "{at}: the worktree line is one line, {height:?} tall: {line:?}"
            );
            let shown = PATH_LINE_SHOWN
                .with_borrow(Clone::clone)
                .expect("the worktree line is drawn");
            let path = shown
                .strip_prefix("Local worktree: ")
                .unwrap_or_else(|| panic!("{at}: {shown:?} starts with its label"));
            let measure = |cx: &mut VisualTestContext, text: &str| {
                cx.update(|window, _| {
                    crate::views::laid_out_text_width(
                        crate::views::shaped_width(
                            text,
                            appearance::ui_text(12.),
                            FontWeight::NORMAL,
                            window,
                        ),
                        window,
                    )
                })
            };
            let drawn = measure(cx, &shown);
            assert!(
                drawn <= line.size.width - px(1.) + px(0.01),
                "{at}: {shown:?}, {drawn:?} wide, fits its line, {line:?}"
            );
            let kept = format!("{top}/…/compact");
            let fits = measure(cx, &format!("Local worktree: {kept}")) <= line.size.width - px(1.);
            assert!(
                path != whole
                    && path.ends_with("/compact")
                    && if fits {
                        path.starts_with(&format!("{top}/…/"))
                    } else {
                        top.starts_with(path.trim_end_matches("…/compact"))
                    },
                "{at}: {path:?} keeps {top} and compact, as {kept:?} where it fits"
            );
            let named = written::get("github-worktree")
                .map(|line| line.node)
                .and_then(|node| node.label().or(node.value()).map(str::to_owned));
            assert_eq!(
                named,
                Some(format!("Local worktree: {whole}")),
                "{at}: the line's accessible name holds the whole path"
            );
            let tooltip = crate::views::CONTROL_TOOLTIPS
                .with(|tooltips| tooltips.borrow().get("github-worktree").cloned());
            assert_eq!(
                tooltip,
                Some(format!("Local worktree: {whole}")),
                "{at}: the line's tooltip holds the whole path"
            );
            checked += 1;
        };
        let walked =
            walk_connection_reveals_with(cx, long_path_window, &[13, 17, 18], [440.], check).await;
        assert_eq!(checked, 3, "the line is checked at each size");
        assert_every_help_line_whole(&walked);
    }

    /// A window drawing one [`PathLine`] of a long path, `width` wide, in `family` where one
    /// is given and in the window's font otherwise.
    struct PathLineView {
        width: Pixels,
        family: Option<SharedString>,
    }
    impl Render for PathLineView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .w(self.width)
                .text_size(px(12.))
                .when_some(self.family.clone(), |line, family| line.font_family(family))
                .child(PathLine {
                    prefix: "Local worktree: ",
                    path: "/home/someone/projects/a-repository-path-long-enough-to-elide/compact"
                        .into(),
                    size: px(12.),
                })
        }
    }

    /// A window drawing a [`PathLine`] ([`PathLineView`]), 300 px wide in its own font.
    fn path_line_window(cx: &mut TestAppContext) -> (Entity<PathLineView>, &mut VisualTestContext) {
        crate::views::tests::init_app(cx);
        cx.add_window_view(|_, _| PathLineView {
            width: px(300.),
            family: None,
        })
    }

    /// Draws `view` `width` wide in `family`, and returns how many times a path line's text
    /// has been worked out.
    fn draw_path_line(
        view: &Entity<PathLineView>,
        width: f32,
        family: Option<&'static str>,
        cx: &mut VisualTestContext,
    ) -> usize {
        view.update(cx, |view, cx| {
            view.width = px(width);
            view.family = family.map(Into::into);
            cx.notify();
        });
        cx.run_until_parked();
        PATH_LINE_MEASURED.with(std::cell::Cell::get)
    }

    /// A [`PathLine`] drawn in another font works its text out again in that font, as the
    /// header's identity does ([`crate::views::LineKey`]), and a frame at rest works out
    /// nothing. Keyed on its size, scale, width and path alone, it kept the text it had
    /// measured in the font before.
    #[gpui::test]
    async fn a_path_line_in_another_font_is_measured_in_it(cx: &mut TestAppContext) {
        // A GPUI test's seeds share the thread and its caches.
        PATH_LINES.with_borrow_mut(|kept| *kept = crate::views::Measured::new());
        let (view, cx) = path_line_window(cx);
        let before = draw_path_line(&view, 300., None, cx);
        assert_eq!(
            draw_path_line(&view, 300., None, cx),
            before,
            "a frame at rest works out nothing"
        );
        assert_eq!(
            draw_path_line(&view, 300., Some("Desktop Mono"), cx),
            before + 1,
            "a line in another font is worked out in it"
        );
    }

    /// [`PathLine`]s drawn at two widths in turn, as two windows with the GitHub panel drawn
    /// at different widths are, work their text out once for each width and keep both
    /// ([`crate::views::Measured`]). A cache of one slot worked each line out again on the
    /// other's frames, every frame.
    #[gpui::test]
    async fn path_lines_at_two_widths_drawn_in_turn_are_measured_once_each(
        cx: &mut TestAppContext,
    ) {
        // A GPUI test's seeds share the thread and its caches.
        PATH_LINES.with_borrow_mut(|kept| *kept = crate::views::Measured::new());
        let before = PATH_LINE_MEASURED.with(std::cell::Cell::get);
        let (view, cx) = path_line_window(cx);
        for _ in 0..3 {
            for width in [300., 360.] {
                draw_path_line(&view, width, None, cx);
            }
        }
        assert_eq!(
            PATH_LINE_MEASURED.with(std::cell::Cell::get) - before,
            2,
            "each width is worked out once"
        );
    }

    /// The connection pane's "Local worktree:" line builds its text once for the panel's
    /// repository, with the panel ([`PathText`]): frames that draw the open pane at rest build
    /// none of it. Each frame built the path's text, the line's label and their shared
    /// strings again.
    #[gpui::test]
    async fn frames_at_rest_build_none_of_the_worktree_lines_text(cx: &mut TestAppContext) {
        use crate::tags::tests::draw;
        let (_fixture, app, cx) = long_path_window(cx);
        crate::focus_ring::sweep::prepare(&app, cx);
        open_panel(&app, cx).await;
        crate::copy_feedback::tests::click(cx, "github-account-options");
        cx.run_until_parked();
        draw(cx);
        assert!(
            cx.debug_bounds("github-worktree").is_some(),
            "the open pane draws the worktree line"
        );
        PATH_TEXT_BUILT.with(|built| built.set(0));
        draw(cx);
        draw(cx);
        assert_eq!(
            PATH_TEXT_BUILT.with(std::cell::Cell::get),
            0,
            "frames at rest build none of the line's text"
        );
    }

    /// Fails unless each line of the connection help is whole at some stop of `walked`'s walk
    /// at each place it was taken ([`walk_connection_reveals`]).
    fn assert_every_help_line_whole(walked: &[Walked]) {
        let mut places: Vec<&str> = walked.iter().map(|step| step.at.as_str()).collect();
        places.dedup();
        for at in places {
            let steps: Vec<&Walked> = walked.iter().filter(|step| step.at == at).collect();
            let lines = steps.iter().map(|step| step.help).max().unwrap_or(0);
            assert!(lines > 1, "{at}: the help takes lines: {lines}");
            let missing: Vec<usize> = (1..=lines)
                .filter(|line| {
                    let name = format!("github-connection-help line {line}");
                    !steps.iter().any(|step| step.whole.contains(&name))
                })
                .collect();
            assert_eq!(
                missing,
                Vec::<usize>::new(),
                "{at}: each line of the help is whole at some stop of the walk"
            );
        }
    }

    /// Opens the panel over a repository with its connection pane open, Connect enabled (no
    /// request runs and the panel is no offline fixture), in each of `widths` × 490 windows
    /// at each of `sizes`, the body scrolled to its top, and walks Tab onto Connect, on onto
    /// Copy sign-in command and Disconnect, and Shift+Tab back onto Account options, holding
    /// each to the rule of the body's reveals ([`assert_reveal_rule`]). Returns each step
    /// ([`Walked`]).
    async fn walk_connection_reveals(
        cx: &mut TestAppContext,
        sizes: &[u8],
        widths: impl IntoIterator<Item = f32> + Clone,
    ) -> Vec<Walked> {
        walk_connection_reveals_with(cx, repository_window, sizes, widths, |_, _, _| {}).await
    }

    /// [`walk_connection_reveals`] over the repository `window` makes, calling `check` with
    /// the place and the repository's path at each place, once the pane is open at the body's
    /// top.
    async fn walk_connection_reveals_with(
        cx: &mut TestAppContext,
        window: fn(
            &mut TestAppContext,
        ) -> (tempfile::TempDir, Entity<GitTurtle>, &mut VisualTestContext),
        sizes: &[u8],
        widths: impl IntoIterator<Item = f32> + Clone,
        mut check: impl FnMut(&mut VisualTestContext, &str, &std::path::Path),
    ) -> Vec<Walked> {
        use crate::tags::tests::draw;
        let mut walked = Vec::new();
        let (_fixture, app, cx) = window(cx);
        let path = app.read_with(cx, |app, _| {
            app.repository.as_ref().unwrap().path().to_path_buf()
        });
        // Tab's bindings, the installed focus ring and motion at rest.
        crate::focus_ring::sweep::prepare(&app, cx);
        let panel = cx.update(|window, cx| {
            let repo = app.read(cx).repository.clone().unwrap();
            let panel = cx.new(|cx| Panel::new(app.downgrade(), repo, "main".into(), window, cx));
            panel.update(cx, |panel, cx| panel.close(window, cx));
            panel
        });
        let scroll = panel.read_with(cx, |panel, _| panel.review.body_scroll.clone());
        let controls = [
            "github-destination",
            "github-refresh",
            "github-create",
            "github-account-options",
            "github-connect",
            "github-sign-in-help",
            "github-disconnect",
            "github-list-empty",
            "github-previous",
            "github-next",
            "github-recovery-toggle",
        ];
        for &interface in sizes {
            for width in widths.clone() {
                let at = format!("{interface} pt in {width} × 490");
                cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
                cx.simulate_resize(size(px(width), px(490.)));
                draw(cx);
                open_panel(&app, cx).await;
                if !panel.read_with(cx, |panel, _| panel.review.connection_open) {
                    crate::copy_feedback::tests::click(cx, "github-account-options");
                    cx.run_until_parked();
                }
                // Connect is enabled: no request runs and the panel is no offline fixture.
                assert!(
                    panel.read_with(cx, |panel, _| !panel.pending && !panel.review.fixture),
                    "{at}: Connect is enabled"
                );
                scroll.set_offset(point(px(0.), px(0.)));
                draw(cx);
                draw(cx);
                assert!(scroll.max_offset().y > px(0.), "{at}: the body scrolls");
                check(cx, at.as_str(), path.as_path());
                // On through the pane's controls, each a part starting a block, and back up.
                for step in [
                    ("tab", "github-connect"),
                    ("tab", "github-sign-in-help"),
                    ("tab", "github-disconnect"),
                    ("shift-tab", "github-account-options"),
                ] {
                    let before = tab_onto(cx, &scroll, step.0, step.1);
                    let cut = assert_reveal_rule(cx, &scroll, before, step, &controls, &at);
                    let (whole, help) = whole_lines(cx, &controls);
                    walked.push(Walked {
                        at: at.clone(),
                        step,
                        cut,
                        whole,
                        help,
                    });
                }
                cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
                settle_panel(&app, cx).await;
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        walked
    }

    /// Escape closes the panel and focus returns to what opened it, drawn as
    /// keyboard focus is drawn there: the header's Pull requests button, full
    /// or compact, takes the Button ring; from the command palette, opened by
    /// the shortcut or a menu's Command Palette item, the control the palette
    /// opened over takes it back, and with only the window's root focused,
    /// which shows nothing, History's list takes it with its accent outline.
    #[gpui::test]
    async fn escape_returns_visible_focus_to_what_opened_the_panel(cx: &mut TestAppContext) {
        use crate::tags::tests::draw;
        let (_fixture, app, cx) = repository_window(cx);
        cx.update(|window, cx| {
            shortcuts::bind_keys(cx);
            app.update(cx, |app, cx| app.apply_appearance(window, cx));
        });
        cx.simulate_resize(size(px(1480.), px(980.)));
        draw(cx);
        let focused = |cx: &mut VisualTestContext, handle: fn(&GitTurtle) -> &FocusHandle| {
            cx.update(|window, cx| handle(app.read(cx)).is_focused(window))
        };
        let full = cx.debug_bounds("pull-requests").unwrap().size.width;

        // The header's button, which a click leaves unfocused.
        for height in [980., 360.] {
            cx.simulate_resize(size(px(1480.), px(height)));
            draw(cx);
            let compact = cx.debug_bounds("pull-requests").unwrap().size.width < full;
            assert_eq!(compact, height < 980., "the {height} px header's button");
            crate::copy_feedback::tests::click(cx, "pull-requests");
            settle_panel(&app, cx).await;
            assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
            assert!(!focused(cx, |app| &app.pull_requests_focus));
            escape(cx);
            assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
            assert!(
                focused(cx, |app| &app.pull_requests_focus),
                "the {height} px header's Pull requests button takes focus back"
            );
            assert!(ringed(cx, "pull-requests"), "the {height} px header's ring");
        }
        cx.simulate_resize(size(px(1480.), px(980.)));
        draw(cx);

        // The palette, from History's list and from the window's root.
        for root in [false, true] {
            cx.update(|window, cx| {
                let focus = if root {
                    app.read(cx).app_focus.clone()
                } else {
                    app.read(cx).focus.clone()
                };
                window.focus(&focus, cx);
            });
            draw(cx);
            cx.dispatch_action(ShowCommandPalette);
            draw(cx);
            cx.simulate_input("GitHub pull requests");
            cx.simulate_keystrokes("enter");
            settle_panel(&app, cx).await;
            assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
            assert!(!focused(cx, |app| &app.focus));
            escape(cx);
            assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
            assert!(
                focused(cx, |app| &app.focus),
                "from the {} History's list takes focus",
                if root { "root" } else { "list" }
            );
            assert!(outlined(cx, "history-pane"), "History's accent outline");
        }
    }

    /// The offline fixture's notice, as the panel opens with it.
    const OFFLINE: &str = "Offline review fixture · fixed sample data; account connection and \
                           network transport are unavailable in this mode.";
    /// The notice Move fixture head leaves.
    const MOVED: &str = "Fixture head moved remotely. This view and its draft retain the \
                         original commit. Submit to verify stale-head refusal, or Refresh PRs \
                         and reopen to inspect the new head.";

    /// A window on an empty repository and the panel on the offline fixture, closed and warm
    /// with the fixture's notice, for [`open_notice_panel`]. Dialogs open at rest, so a click
    /// lands where the last frame drew its button. Keep the returned directory until the test
    /// ends.
    fn notice_panel(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        Entity<GitTurtle>,
        Entity<Panel>,
        &mut VisualTestContext,
    ) {
        let (fixture, app, cx) = repository_window(cx);
        cx.update(|_, cx| cx.set_reduce_motion(true));
        let panel = cx.update(|window, cx| {
            let repo = app.read(cx).repository.clone().unwrap();
            let panel = cx.new(|cx| Panel::new(app.downgrade(), repo, "main".into(), window, cx));
            panel.update(cx, |panel, cx| {
                panel.review.fixture = true;
                panel.account = Some("offline-reviewer".into());
                panel.notice = Some(OFFLINE.into());
                panel.destination.update(cx, |input, cx| {
                    input.set_value("gitturtle-fixture/native-review", window, cx)
                });
                panel.close(window, cx);
            });
            panel
        });
        (fixture, app, panel, cx)
    }

    /// Opens the warm panel at `interface` pt in a `width` × `height` window, its body at its
    /// top.
    async fn open_notice_panel(
        app: &Entity<GitTurtle>,
        panel: &Entity<Panel>,
        (interface, width, height): (u8, f32, f32),
        cx: &mut VisualTestContext,
    ) {
        use crate::tags::tests::draw;
        cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
        cx.simulate_resize(size(px(width), px(height)));
        draw(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
        settle_panel(app, cx).await;
        let scroll = panel.read_with(cx, |panel, _| panel.review.body_scroll.clone());
        scroll.set_offset(point(px(0.), px(0.)));
        draw(cx);
        draw(cx);
    }

    /// Scrolls the body so the notice's top lies at `top`, as far as the body scrolls.
    fn scroll_notice_to(cx: &mut VisualTestContext, scroll: &ScrollHandle, top: Pixels) {
        let notice = cx.debug_bounds("github-notice").expect("the notice");
        let offset =
            (scroll.offset().y + (top - notice.top())).clamp(-scroll.max_offset().y, px(0.));
        scroll.set_offset(point(px(0.), offset));
        crate::tags::tests::draw(cx);
        crate::tags::tests::draw(cx);
    }

    /// The window's scale: one pixel in AccessKit's physical bounds.
    fn pixel(cx: &mut VisualTestContext) -> f64 {
        f64::from(cx.update(|window, _| window.scale_factor()))
    }

    /// `bounds` in the physical window pixels of AccessKit's bounds.
    fn physical_rect(cx: &mut VisualTestContext, bounds: Bounds<Pixels>) -> gpui::accesskit::Rect {
        let scale = pixel(cx);
        let edge = |value: Pixels| f64::from(f32::from(value)) * scale;
        gpui::accesskit::Rect {
            x0: edge(bounds.left()),
            y0: edge(bounds.top()),
            x1: edge(bounds.right()),
            y1: edge(bounds.bottom()),
        }
    }

    /// Whether two rectangles in physical pixels agree to half a pixel.
    fn same_rect(a: gpui::accesskit::Rect, b: gpui::accesskit::Rect) -> bool {
        [(a.x0, b.x0), (a.y0, b.y0), (a.x1, b.x1), (a.y1, b.y1)]
            .into_iter()
            .all(|(a, b)| (a - b).abs() <= 0.5)
    }

    /// Every node an assistive technology reaches in the window's tree as the next frame
    /// writes it, through AccessKit's filter, the dialog's own node added for the dialog.
    fn reachable_nodes(cx: &mut VisualTestContext) -> Vec<native_accessibility::Reachable> {
        use crate::native_accessibility::{reachable, written};
        written::forget();
        crate::tags::tests::draw(cx);
        let size = cx.update(|window, _| window.viewport_size());
        let window = physical_rect(cx, Bounds::new(point(px(0.), px(0.)), size));
        let tree = written::Assembly::default()
            .node("dialog", gpui::accesskit::Node::new(Role::Dialog), None)
            .everything(Some("dialog"))
            .tree(window, None);
        reachable(tree)
    }

    /// The panel's notice, a status, reports the part of itself the panel's scrolling body
    /// draws, one pixel inside the body at its fold where the body draws less than a pixel of
    /// it, and where the body draws none of it, its layout: where it lies. It reported its
    /// layout where the body cut it, so at 18 pt in a 461 × 490 window AT-SPI gave a notice
    /// the fold cut y 569–731, over Back to repository; then an empty rectangle on the body's
    /// edge where the body drew none of it, which AccessKit keeps as the child past the fold
    /// and AT-SPI reported 0 px tall at y 557; then, half a pixel above the fold, its layout
    /// over Back to repository again, showing. On Linux the notice the body draws none of is
    /// visible but not showing on AT-SPI, at the extents of its layout, and one it draws some
    /// of shows: reported where it lies and showing, it read as shown over Back to repository.
    #[gpui::test]
    async fn the_notice_reports_the_part_the_panel_draws(cx: &mut TestAppContext) {
        use crate::native_accessibility::written;
        let (_fixture, app, panel, cx) = notice_panel(cx);
        let scroll = panel.read_with(cx, |panel, _| panel.review.body_scroll.clone());
        // The notice's node, the part of the body the window draws and the notice's layout,
        // as the next frame writes them.
        let measure = |cx: &mut VisualTestContext| {
            written::forget();
            crate::tags::tests::draw(cx);
            let body = native_accessibility::clipping_node("github-panel")
                .expect("the body clips what it holds")
                .bounds()
                .expect("bounds");
            let node = written::get("github-notice")
                .expect("the notice writes its node")
                .node;
            assert_eq!(node.role(), Role::Status);
            let layout = cx.debug_bounds("github-notice").expect("the notice");
            (
                node.bounds().expect("bounds"),
                body,
                physical_rect(cx, layout),
            )
        };
        let mut cut = 0;
        for sizes in [(13, 461., 490.), (18, 461., 490.), (13, 1480., 980.)] {
            let (interface, width, height) = sizes;
            let at = format!("{interface} pt in {width} × {height}");
            open_notice_panel(&app, &panel, sizes, cx).await;
            let scale = pixel(cx);
            let (notice, body, layout) = measure(cx);
            let (top, bottom) = (layout.y0.max(body.y0), layout.y1.min(body.y1));
            #[cfg(target_os = "linux")]
            {
                use crate::native_accessibility::atspi_state_tests::{next_frame, reached};
                use accesskit_atspi_common::State;
                let found = reached(next_frame(cx));
                // The notice's own node, not the window's announcement of it,
                // which the test's tree records without bounds.
                let shown = found
                    .iter()
                    .find(|found| {
                        found.name == OFFLINE
                            && found.extents != accesskit_atspi_common::Rect::INVALID
                    })
                    .unwrap_or_else(|| panic!("{at}: AT-SPI reaches the notice"));
                assert_eq!(
                    (shown.extents.y, shown.extents.y + shown.extents.height),
                    (notice.y0 as i32, notice.y1 as i32),
                    "{at}: AT-SPI reports the notice at its node's bounds"
                );
                assert_eq!(
                    (
                        shown.state.contains(State::Visible),
                        shown.state.contains(State::Showing)
                    ),
                    (true, bottom > top),
                    "{at}: the notice is visible, and shows only where the body draws it: {:?}",
                    shown.state
                );
            }
            if bottom - top >= scale {
                assert!(
                    (notice.y0 - top).abs() <= 0.5 && (notice.y1 - bottom).abs() <= 0.5,
                    "{at}: the notice reports {notice:?}, not the part of it the body draws, \
                     {top}–{bottom}"
                );
            } else if bottom > top {
                assert!(
                    (notice.y1 - notice.y0 - scale).abs() <= 0.01
                        && notice.y0 >= body.y0 - 0.01
                        && notice.y1 <= body.y1 + 0.01,
                    "{at}: the notice the body draws less than a pixel of reports {notice:?}, \
                     not one pixel inside the body"
                );
            } else {
                assert!(
                    same_rect(notice, layout),
                    "{at}: the notice the body draws none of reports {notice:?}, not where it \
                     lies, {layout:?}"
                );
            }
            if layout.y0 >= body.y1 {
                // Below the fold. Scrolled so the fold cuts it 10 px below its top, it reports
                // those 10 px, while its layout runs on past the fold.
                let fold = px((body.y1 / scale) as f32);
                scroll_notice_to(cx, &scroll, fold - px(10.));
                let (notice, body, layout) = measure(cx);
                assert!(
                    layout.y1 > body.y1 + scale,
                    "{at}: the fold cuts the notice"
                );
                assert!(
                    (notice.y0 - layout.y0).abs() <= 0.5
                        && (notice.y1 - body.y1).abs() <= 0.5
                        && (notice.y1 - notice.y0 - 10. * scale).abs() <= 0.5,
                    "{at}: the notice the fold cuts reports {notice:?}, not its 10 px above \
                     the fold at {}",
                    body.y1
                );
                // Half a pixel above the fold, it reports one pixel inside the body at the
                // fold, where the body draws it, and shows; its layout runs on over Back to
                // repository.
                scroll_notice_to(cx, &scroll, fold - px(0.5));
                let (notice, body, layout) = measure(cx);
                assert!(
                    layout.y0 < body.y1 && body.y1 - layout.y0 < scale,
                    "{at}: the body draws less than a pixel of the notice {layout:?}, above its \
                     fold at {}",
                    body.y1
                );
                assert!(
                    (notice.y0 - (body.y1 - scale)).abs() <= 0.01
                        && (notice.y1 - body.y1).abs() <= 0.01,
                    "{at}: the notice half a pixel above the fold reports {notice:?}, not one \
                     pixel inside the body at its fold, {}",
                    body.y1
                );
                #[cfg(target_os = "linux")]
                {
                    use crate::native_accessibility::atspi_state_tests::{next_frame, reached};
                    use accesskit_atspi_common::State;
                    let found = reached(next_frame(cx));
                    // The notice's own node, not the window's announcement.
                    let shown = found
                        .iter()
                        .find(|found| {
                            found.name == OFFLINE
                                && found.extents != accesskit_atspi_common::Rect::INVALID
                        })
                        .unwrap_or_else(|| panic!("{at}: AT-SPI reaches the notice"));
                    assert_eq!(
                        (shown.extents.y, shown.extents.y + shown.extents.height),
                        (notice.y0 as i32, notice.y1 as i32),
                        "{at}: AT-SPI reports the notice half a pixel above the fold inside the \
                         body"
                    );
                    assert!(
                        shown.state.contains(State::Visible | State::Showing),
                        "{at}: the notice the body draws half a pixel of shows: {:?}",
                        shown.state
                    );
                }
                cut += 1;
            }
            escape(cx);
        }
        assert!(
            cut > 0,
            "the body's fold lies above the notice at some size"
        );
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// No node an assistive technology reaches in the panel reports an empty rectangle, or one
    /// less than a pixel across, at 13 and 18 pt in a 461 × 490 window: with the notice, which
    /// at 18 pt lies below the body's fold, the child past it that AccessKit keeps, and
    /// reports where it lies; without it, where the pull request list is that child; and with
    /// a failure's alert, which is announced at once as it appears. Each undrawn one reported
    /// an empty rectangle on the body's edge (y 557 at 18 pt), as the pager's "Page 1 · up to
    /// 50 PRs" did at 13 pt. The alert's own node is no live region: the window's alert says it.
    /// The list past the fold reports a pixel beyond a corner, so no platform's filter reaches
    /// its pull requests: reported at its layout, it kept #42 and #43 reachable, showing on
    /// AT-SPI and activatable, though the body draws neither.
    #[gpui::test]
    async fn no_panel_node_reports_an_empty_rectangle(cx: &mut TestAppContext) {
        use crate::native_accessibility::written;
        const FAILURE: &str = "Could not read repository remotes and saved drafts.";
        let (_fixture, app, panel, cx) = notice_panel(cx);
        let reach = |cx: &mut VisualTestContext, at: &str| {
            let found = reachable_nodes(cx);
            let scale = pixel(cx);
            for node in &found {
                if let Some(bounds) = node.bounds {
                    assert!(
                        bounds.x1 - bounds.x0 >= scale - 0.01
                            && bounds.y1 - bounds.y0 >= scale - 0.01,
                        "{at}: {:?} {:?} is reached at {bounds:?}, less than a pixel across",
                        node.role,
                        node.name
                    );
                }
            }
            found
        };
        // A node reached past the fold of its clipping ancestor.
        let past_fold = |node: &native_accessibility::Reachable| {
            node.bounds
                .zip(node.clip)
                .is_some_and(|(bounds, clip)| bounds.y0 >= clip.y1 - 0.01)
        };
        for sizes in [(13, 461., 490.), (18, 461., 490.)] {
            let (interface, width, height) = sizes;
            let at = format!("{interface} pt in {width} × {height}");
            cx.update(|_, cx| {
                panel.update(cx, |panel, _| {
                    panel.notice = Some(OFFLINE.into());
                    panel.error = None;
                })
            });
            open_notice_panel(&app, &panel, sizes, cx).await;
            let found = reach(cx, &format!("{at}, with the notice"));
            // The notice's own line, no live region, not the window's
            // announcement of it.
            let notice = found.iter().find(|node| {
                node.name.as_deref() == Some(OFFLINE) && node.live == gpui::accesskit::Live::Off
            });
            if interface == 18 {
                let notice = notice.unwrap_or_else(|| panic!("{at}: AccessKit keeps the notice"));
                assert!(
                    past_fold(notice),
                    "{at}: the notice reports where it lies, past the fold: {notice:?}"
                );
            }

            cx.update(|_, cx| {
                panel.update(cx, |panel, cx| {
                    panel.notice = None;
                    if interface == 18 {
                        let pull = github::review::fixture_pull(false);
                        let mut other = pull.clone();
                        other.number = 43;
                        panel.rows = vec![pull, other];
                    }
                    cx.notify();
                })
            });
            let found = reach(cx, &format!("{at}, without the notice"));
            if interface == 18 {
                assert!(
                    found
                        .iter()
                        .any(|node| node.role == Role::ScrollView && past_fold(node)),
                    "{at}: the pull request list is kept past the fold where it lies: {found:?}"
                );
                // The body draws none of the list, which reports a pixel beyond a corner,
                // outside which its rows lie, so no platform's filter reaches a row.
                let scale = pixel(cx);
                let list = native_accessibility::clipping_node("github-pr-list")
                    .and_then(|node| node.bounds())
                    .expect("the list clips its rows");
                let layout = cx.debug_bounds("github-pr-list").expect("the list");
                let layout = physical_rect(cx, layout);
                assert!(
                    native_accessibility::beyond_a_corner(list, layout, scale),
                    "{at}: the list the body draws none of reports {list:?}, not a pixel \
                     beyond a corner of its {layout:?}"
                );
                let rows = panel.read_with(cx, |panel, _| {
                    panel
                        .rows
                        .iter()
                        .map(|pull| format!("#{} · {}", pull.number, pull.title))
                        .collect::<Vec<_>>()
                });
                assert!(
                    !rows.is_empty() && cx.debug_bounds("github-pr-42").is_some(),
                    "{at}: the list lays out its pull requests"
                );
                for row in &rows {
                    assert!(
                        !found.iter().any(|node| node.name.as_deref() == Some(row)),
                        "{at}: {row}, which the body does not draw, is out of reach: {found:?}"
                    );
                }
                #[cfg(target_os = "linux")]
                {
                    use crate::native_accessibility::atspi_state_tests::{next_frame, reached};
                    use accesskit_atspi_common::State;
                    for node in reached(next_frame(cx))
                        .iter()
                        .filter(|node| rows.contains(&node.name))
                    {
                        assert!(
                            node.state.contains(State::Visible)
                                && !node.state.contains(State::Showing),
                            "{at}: AT-SPI reports {} showing: {:?}",
                            node.name,
                            node.state
                        );
                    }
                }
                cx.update(|_, cx| panel.update(cx, |panel, _| panel.rows.clear()));
            }

            #[cfg(target_os = "linux")]
            let mut bus = {
                use crate::native_accessibility::atspi_state_tests::{Bus, next_frame};
                Bus::new(next_frame(cx))
            };
            cx.update(|_, cx| {
                panel.update(cx, |panel, cx| {
                    panel.error = Some(FAILURE.into());
                    cx.notify();
                })
            });
            // The frame that draws the failure's line, before the window's alert.
            let found = reach(cx, &format!("{at}, with a failure"));
            assert_eq!(
                written::announced("github-error"),
                None,
                "{at}: the failure's line is no live region of its own"
            );
            assert!(
                found
                    .iter()
                    .any(|node| node.role == Role::Status && node.name.as_deref() == Some(FAILURE)),
                "{at}: the failure's line is in reach"
            );
            crate::tags::tests::draw(cx);
            assert_eq!(
                crate::copy_feedback::tests::alerted(cx)
                    .map(|(_, text)| text)
                    .as_deref(),
                Some(FAILURE),
                "{at}: the window's alert says the failure"
            );
            #[cfg(target_os = "linux")]
            {
                use crate::native_accessibility::atspi_state_tests::next_frame;
                bus.update(next_frame(cx));
                assert_eq!(
                    bus.announced()
                        .iter()
                        .filter(|text| *text == FAILURE)
                        .count(),
                    1,
                    "{at}: AT-SPI announces the failure once, from the window's alert"
                );
            }
            cx.update(|_, cx| panel.update(cx, |panel, _| panel.error = None));
            escape(cx);
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The window's announcement says the panel's notice as the panel opens with it, again as
    /// it reopens with it unchanged, and as it changes, wherever the body has scrolled it:
    /// Move fixture head changes it from the banner while, at 18 pt in a 461 × 490 window, it
    /// lies below the fold. Scrolling the body to it announces nothing, and the notice is no
    /// live region of its own. A copy button keeps reading Copied for its 1.5 s through a
    /// notice, which announced as a confirmation without a button would end it at once.
    #[gpui::test]
    async fn the_window_announces_the_panels_notice(cx: &mut TestAppContext) {
        use crate::copy_feedback::{
            RESTORE_AFTER,
            tests::{Shows, announced, click, shows, wait},
        };
        use crate::native_accessibility::written;
        use crate::tags::tests::draw;
        let (_fixture, app, panel, cx) = notice_panel(cx);
        let scroll = panel.read_with(cx, |panel, _| panel.review.body_scroll.clone());
        for sizes in [(13, 461., 490.), (18, 461., 490.)] {
            let (interface, width, height) = sizes;
            let at = format!("{interface} pt in {width} × {height}");
            cx.update(|_, cx| {
                panel.update(cx, |panel, _| {
                    panel.notice = Some(OFFLINE.into());
                    panel.review.fixture_moved = false;
                })
            });
            open_notice_panel(&app, &panel, sizes, cx).await;
            assert_eq!(
                announced(cx).map(|(_, text)| text).as_deref(),
                Some(OFFLINE),
                "{at}: the panel announces the notice it opens with"
            );
            assert_eq!(
                written::announced("github-notice"),
                None,
                "{at}: the notice is no live region of its own"
            );

            click(cx, "github-fixture-move");
            draw(cx);
            draw(cx);
            let (heard, notice) = announced(cx).expect("an announcement");
            assert_eq!(notice, MOVED, "{at}: the changed notice is announced");
            let body = cx.debug_bounds("github-panel").expect("the body");
            let shown = cx.debug_bounds("github-notice").expect("the notice");
            assert_eq!(
                shown.top() < body.bottom(),
                interface == 13,
                "{at}: the body draws the changed notice at 13 pt only"
            );

            scroll_notice_to(cx, &scroll, body.top());
            assert_eq!(
                announced(cx).map(|(id, _)| id),
                Some(heard.clone()),
                "{at}: scrolling the notice into view announces nothing"
            );

            // Closed until the announcement has gone, and reopened with the notice unchanged.
            escape(cx);
            wait(cx, RESTORE_AFTER);
            assert_eq!(announced(cx), None, "{at}: the announcement has gone");
            cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
            settle_panel(&app, cx).await;
            draw(cx);
            draw(cx);
            let (again, notice) = announced(cx).expect("an announcement");
            assert_eq!(
                (notice.as_str(), again != heard),
                (MOVED, true),
                "{at}: the reopened panel announces its notice afresh"
            );
            escape(cx);
        }

        // A copy, then a notice.
        cx.update(|_, cx| {
            panel.update(cx, |panel, _| {
                panel.notice = Some(OFFLINE.into());
                panel.review.fixture_moved = false;
            })
        });
        open_notice_panel(&app, &panel, (13, 1480., 980.), cx).await;
        click(cx, "github-account-options");
        cx.run_until_parked();
        draw(cx);
        click(cx, "github-sign-in-help");
        assert_eq!(shows(cx, "github-sign-in-help"), Shows::Copied);
        click(cx, "github-fixture-move");
        draw(cx);
        draw(cx);
        assert_eq!(
            announced(cx).map(|(_, text)| text).as_deref(),
            Some(MOVED),
            "the notice is announced"
        );
        assert_eq!(
            shows(cx, "github-sign-in-help"),
            Shows::Copied,
            "the sign-in command still reads Copied"
        );
        wait(cx, RESTORE_AFTER);
        assert_eq!(shows(cx, "github-sign-in-help"), Shows::Rest);
        assert_eq!(announced(cx), None);
        escape(cx);
    }

    /// The window's alert says the panel's failure and refused input at once, as they appear
    /// or change, wherever the body has scrolled their lines, which are no live regions of
    /// their own. A failure the body draws is announced once, from the window's alert alone,
    /// and drawing the panel again says nothing more. A failure landing while the body has
    /// scrolled far past its line, which AccessKit's filter leaves out, was never announced.
    /// A copy button keeps reading Copied for its 1.5 s through an alert. The lines are
    /// statuses rather than alerts: AT-SPI reports a line becoming showing as the body
    /// scrolls it into view, and Orca presents an alert, a notification there, when it does.
    #[gpui::test]
    async fn the_window_alerts_the_panels_failures(cx: &mut TestAppContext) {
        use crate::copy_feedback::{
            RESTORE_AFTER,
            tests::{Shows, alerted, click, shows, wait},
        };
        use crate::native_accessibility::written;
        use crate::tags::tests::draw;
        const FAILURE: &str = "Could not read repository remotes and saved drafts.";
        const LATER: &str = "Conversations could not be loaded.";
        let (_fixture, app, panel, cx) = notice_panel(cx);
        let scroll = panel.read_with(cx, |panel, _| panel.review.body_scroll.clone());
        let fail = |cx: &mut VisualTestContext, error: Option<&str>| {
            cx.update(|_, cx| {
                panel.update(cx, |panel, cx| {
                    panel.error = error.map(str::to_owned);
                    cx.notify();
                })
            });
        };

        // At 13 pt in a 461 × 490 window the body draws the failure's line.
        open_notice_panel(&app, &panel, (13, 461., 490.), cx).await;
        #[cfg(target_os = "linux")]
        let mut bus = {
            use crate::native_accessibility::atspi_state_tests::{Bus, next_frame};
            Bus::new(next_frame(cx))
        };
        fail(cx, Some(FAILURE));
        draw(cx);
        draw(cx);
        let body = cx.debug_bounds("github-panel").expect("the body");
        let line = cx.debug_bounds("github-error").expect("the failure's line");
        assert!(
            line.top() < body.bottom() && line.bottom() > body.top(),
            "the body draws the failure's line"
        );
        let (heard, text) = alerted(cx).expect("an alert");
        assert_eq!(text, FAILURE, "the window's alert says the failure");
        assert_eq!(
            written::announced("github-error"),
            None,
            "the failure's line is no live region of its own"
        );
        assert_eq!(
            written::get("github-error").map(|line| line.node.role()),
            Some(Role::Status),
            "the failure's line is a status, no alert"
        );
        #[cfg(target_os = "linux")]
        {
            use crate::native_accessibility::atspi_state_tests::{next_frame, reached};
            use accesskit_atspi_common::Role as AtspiRole;
            let frame = next_frame(cx);
            let roles: Vec<AtspiRole> = reached(frame.clone())
                .into_iter()
                .filter(|node| node.name == FAILURE)
                .map(|node| node.role)
                .collect();
            assert!(
                roles.contains(&AtspiRole::StatusBar)
                    && roles
                        .iter()
                        .filter(|role| **role == AtspiRole::Notification)
                        .count()
                        <= 1,
                "the failure's line is a status bar on AT-SPI, and only the window's alert a \
                 notification, which Orca presents as it becomes showing: {roles:?}"
            );
            bus.update(frame);
            assert_eq!(
                bus.announced(),
                vec![FAILURE.to_owned()],
                "AT-SPI announces the failure once"
            );
        }
        cx.update(|_, cx| panel.update(cx, |_, cx| cx.notify()));
        draw(cx);
        draw(cx);
        assert_eq!(
            alerted(cx).map(|(id, _)| id),
            Some(heard),
            "drawing the panel again alerts nothing new"
        );
        #[cfg(target_os = "linux")]
        {
            use crate::native_accessibility::atspi_state_tests::next_frame;
            bus.update(next_frame(cx));
            assert_eq!(bus.announced(), Vec::<String>::new(), "nor on AT-SPI");
        }

        // Refused input is said at once too.
        cx.update(|_, cx| {
            panel.update(cx, |panel, cx| {
                panel.draft_issue = Some(DraftIssue::Destination);
                cx.notify();
            })
        });
        draw(cx);
        draw(cx);
        assert_eq!(
            alerted(cx).map(|(_, text)| text).as_deref(),
            Some(DraftIssue::Destination.message()),
            "the window's alert says the refused input"
        );
        assert_eq!(written::announced("github-draft-validation"), None);
        assert_eq!(
            written::get("github-draft-validation").map(|line| line.node.role()),
            Some(Role::Status),
            "the refused input's line is a status, no alert"
        );
        escape(cx);
        cx.update(|_, cx| {
            panel.update(cx, |panel, _| {
                panel.error = None;
                panel.draft_issue = None;
            })
        });

        // A PR's Overview at 18 pt, the body scrolled to its end, far below the failure's line.
        cx.update(|_, cx| {
            panel.update(cx, |panel, _| {
                panel.pull = Some(github::review::fixture_pull(false));
                panel.review.section = Section::Overview;
                panel.review.show_list = false;
            })
        });
        open_notice_panel(&app, &panel, (18, 461., 490.), cx).await;
        scroll.set_offset(point(px(0.), -scroll.max_offset().y));
        draw(cx);
        draw(cx);
        fail(cx, Some(LATER));
        let found = reachable_nodes(cx);
        let body = cx.debug_bounds("github-panel").expect("the body");
        let line = cx.debug_bounds("github-error").expect("the failure's line");
        assert!(
            line.bottom() < body.top(),
            "the body has scrolled past the failure's line: {line:?} above {body:?}"
        );
        assert!(
            !found
                .iter()
                .any(|node| node.bounds.is_some() && node.name.as_deref() == Some(LATER)),
            "AccessKit's filter leaves out the failure's line"
        );
        draw(cx);
        assert_eq!(
            alerted(cx).map(|(_, text)| text).as_deref(),
            Some(LATER),
            "the window's alert says the failure the body has scrolled past"
        );
        escape(cx);
        cx.update(|_, cx| {
            panel.update(cx, |panel, _| {
                panel.error = None;
                panel.pull = None;
            })
        });

        // A copy, then a failure.
        open_notice_panel(&app, &panel, (13, 1480., 980.), cx).await;
        click(cx, "github-account-options");
        cx.run_until_parked();
        draw(cx);
        click(cx, "github-sign-in-help");
        assert_eq!(shows(cx, "github-sign-in-help"), Shows::Copied);
        fail(cx, Some(FAILURE));
        draw(cx);
        draw(cx);
        assert_eq!(alerted(cx).map(|(_, text)| text).as_deref(), Some(FAILURE));
        assert_eq!(
            shows(cx, "github-sign-in-help"),
            Shows::Copied,
            "the sign-in command still reads Copied"
        );
        wait(cx, RESTORE_AFTER);
        assert_eq!(shows(cx, "github-sign-in-help"), Shows::Rest);
        assert_eq!(alerted(cx), None);
        escape(cx);
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// While a reply is open, the failure of what was asked from its composer, or that Cancel
    /// stopped it ([`Origin::Reply`]), stands in the reply's own composer, under the draft and
    /// above Review reply…, where the reader writing the reply looks and where a sent reply's
    /// refusal answers: beside the draft, never over it, whole in view with it wherever the
    /// composer fits the body, at 13 pt in a 461 × 490 window and 18 pt in a 1,480 × 800 one.
    /// The line at the panel's top, far above the conversation, is not drawn then. The window
    /// says the failure once, as an alert, and the cancel politely.
    #[gpui::test]
    async fn a_failure_stands_in_the_reply_composer(cx: &mut TestAppContext) {
        use crate::copy_feedback::tests::{alerted, announced};
        use crate::tags::tests::draw;
        const FAILURE: &str = "The captured reply context is unavailable or changed.";
        let (_fixture, app, panel, cx) = notice_panel(cx);
        let pull = github::review::fixture_pull(false);
        let identity = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let page = Client(UiTransport::Fixture { moved: false })
            .review_threads(&identity, None, &OperationControl::default())
            .unwrap();
        let target = page.items[0].target.clone();
        cx.update(|_, cx| {
            panel.update(cx, |panel, _| {
                panel.pull = Some(pull);
                panel.review.show_list = false;
                panel.review.section = Section::Overview;
                panel.receive_threads(page, None);
                panel.conversations.active = Some(target.clone());
            })
        });
        let set = |cx: &mut VisualTestContext, error: Option<&str>, cancelled| {
            cx.update(|_, cx| {
                panel.update(cx, |panel, cx| {
                    // As a request from the reply's composer answers.
                    panel.origin = Origin::Reply(Box::new(target.clone()));
                    panel.error = error.map(str::to_owned);
                    panel.cancelled = cancelled;
                    cx.notify();
                })
            });
            draw(cx);
            draw(cx);
        };
        let beside_the_draft = |cx: &mut VisualTestContext, id: &'static str, at: &str| {
            let body = cx.debug_bounds("github-panel").expect("the body");
            let composer = cx
                .debug_bounds("github-reply-composer")
                .expect("the reply's composer");
            let context = cx
                .debug_bounds("github-reply-context")
                .expect("the reply's context");
            let line = cx.debug_bounds(id).expect("the line");
            let draft = cx.update(|_, _| appearance::ui_size(100.));
            assert!(
                composer.top() <= line.top() && line.bottom() <= composer.bottom(),
                "{at}: {id} {line:?} stands in the composer {composer:?}"
            );
            assert!(
                line.top() >= context.bottom() + draft,
                "{at}: {id} {line:?} lies under the draft, below {context:?} by its {draft:?}"
            );
            assert!(
                body.top() <= context.bottom() && line.bottom() <= body.bottom(),
                "{at}: {id} {line:?} is whole in view with the draft below {context:?} in \
                 {body:?}"
            );
            assert!(
                cx.debug_bounds("github-error").is_none()
                    && cx.debug_bounds("github-cancelled").is_none(),
                "{at}: the panel's top line is not drawn"
            );
        };
        for (interface, width, height) in [(13, 461., 490.), (18, 1480., 800.)] {
            let at = format!("{interface} pt in {width} × {height}");
            open_notice_panel(&app, &panel, (interface, width, height), cx).await;
            // The reader is writing the reply.
            cx.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    // As Reply does, which reveals the draft.
                    *panel.review.revealed_focus.borrow_mut() = Default::default();
                    let input = panel.conversations.input.read(cx).focus_handle(cx);
                    window.focus(&input, cx);
                    cx.notify();
                })
            });
            draw(cx);
            draw(cx);

            set(cx, Some(FAILURE), None);
            beside_the_draft(cx, "github-reply-error", &at);
            let (heard, text) = alerted(cx).expect("an alert");
            assert_eq!(text, FAILURE, "{at}: the window says the failure");
            cx.update(|_, cx| panel.update(cx, |_, cx| cx.notify()));
            draw(cx);
            draw(cx);
            assert_eq!(
                alerted(cx).map(|(id, _)| id),
                Some(heard),
                "{at}: and says it once"
            );

            let line = RequestKind::Compare.cancelled(false);
            set(cx, None, line);
            beside_the_draft(cx, "github-reply-cancelled", &at);
            assert_eq!(
                announced(cx).map(|(_, text)| text).as_deref(),
                line,
                "{at}: the cancel is said politely"
            );
            escape(cx);
            set(cx, None, None);
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// While a reply is open, an outcome answers on the control the person touched
    /// ([`Origin`]): a PR's Changes, asked for from Overview's Open captured local comparison
    /// and stopped by the panel's Cancel, says it was cancelled on the panel's top line, above
    /// the conversation, where every outcome once answered under the reply's draft while a
    /// conversation was open; a reply's Send that is refused still answers in the reply's
    /// composer, under its draft.
    #[gpui::test]
    async fn outcomes_answer_where_they_were_asked_for_while_a_reply_is_open(
        cx: &mut TestAppContext,
    ) {
        use crate::copy_feedback::tests::click;
        use crate::views::tests::draw;
        let (_fixture, app, panel, cx) = busy_fixture_panel(cx).await;
        // Opened at rest, so a click lands where the last frame drew its button.
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
        let pull = local_pull(&app, cx);
        let identity = github::review::fixture_pull(false)
            .capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let page = Client(UiTransport::Fixture { moved: false })
            .review_threads(&identity, None, &OperationControl::default())
            .unwrap();
        let target = page.items[0].target.clone();
        cx.update(|window, cx| {
            cx.set_reduce_motion(true);
            panel.update(cx, |panel, cx| {
                panel.pull = Some(pull);
                panel.review.show_list = false;
                panel.review.section = Section::Overview;
                panel.receive_threads(page, None);
                panel.conversations.active = Some(target.clone());
                panel
                    .conversations
                    .input
                    .update(cx, |input, cx| input.set_value("Exact reply", window, cx));
            })
        });
        open_notice_panel(&app, &panel, (13, 1480., 980.), cx).await;
        let composer = cx
            .debug_bounds("github-reply-composer")
            .expect("the reply is open");

        // Open captured local comparison, held as a large comparison holds it, then the
        // panel's Cancel.
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.changes(window, cx)));
        draw(cx);
        click(cx, "github-cancel");
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        settle_panel(&app, cx).await;
        draw(cx);
        draw(cx);
        assert_eq!(
            panel.read_with(cx, |panel, _| (panel.error.clone(), panel.cancelled)),
            (None, RequestKind::Compare.cancelled(false)),
            "Changes was cancelled"
        );
        let line = cx
            .debug_bounds("github-cancelled")
            .expect("the cancel answers on the panel's top line");
        assert!(
            line.bottom() <= composer.top(),
            "the line {line:?} stands above the conversation, not in the reply {composer:?}"
        );
        assert!(
            cx.debug_bounds("github-reply-cancelled").is_none(),
            "nothing answers under the draft"
        );

        // The reply's Send, refused: the fixture's head moved.
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.review.fixture_moved = true;
                panel.confirm = Some(Confirmation {
                    account: target.account.clone(),
                    action: Action::Reply {
                        target: target.clone(),
                        body: "Exact reply".into(),
                    },
                });
                panel.submit(window, cx);
            })
        });
        app.read_with(cx, |app, _| app.operations.drain());
        settle_panel(&app, cx).await;
        draw(cx);
        draw(cx);
        assert!(
            panel.read_with(cx, |panel, _| panel.error.is_some()),
            "the reply is refused"
        );
        let composer = cx
            .debug_bounds("github-reply-composer")
            .expect("the reply stays open");
        let line = cx
            .debug_bounds("github-reply-error")
            .expect("the refusal answers in the reply's composer");
        assert!(
            composer.top() <= line.top() && line.bottom() <= composer.bottom(),
            "the refusal {line:?} stands in the composer {composer:?}"
        );
        assert!(
            cx.debug_bounds("github-error").is_none()
                && cx.debug_bounds("github-cancelled").is_none(),
            "the panel's top line is not drawn"
        );
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
    }

    /// A reply refused on one conversation stays that conversation's ([`Origin::Reply`]):
    /// Reply on another opens its composer with nothing under its draft, and the refusal,
    /// whose composer is no longer drawn, stands on the panel's top line, said once. It once
    /// stood under the other conversation's draft, which no one had reviewed.
    #[gpui::test]
    async fn a_refused_reply_stays_with_its_conversation(cx: &mut TestAppContext) {
        use crate::copy_feedback::tests::alerted;
        use crate::tags::tests::draw;
        let (_fixture, app, panel, cx) = notice_panel(cx);
        let pull = github::review::fixture_pull(false);
        let identity = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let page = Client(UiTransport::Fixture { moved: false })
            .review_threads(&identity, None, &OperationControl::default())
            .unwrap();
        let open = page
            .items
            .iter()
            .filter(|thread| thread.reply_available())
            .map(|thread| thread.target.clone())
            .collect::<Vec<_>>();
        let [first, second, ..] = &open[..] else {
            panic!("two of the fixture's conversations accept a reply");
        };
        cx.update(|_, cx| {
            panel.update(cx, |panel, _| {
                panel.pull = Some(pull);
                panel.review.show_list = false;
                panel.review.section = Section::Overview;
                panel.receive_threads(page, None);
            })
        });
        open_notice_panel(&app, &panel, (13, 1480., 980.), cx).await;

        // Reply on the first, then Review reply… with nothing written: refused there.
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.begin_reply(first.clone(), window, cx);
                panel.review_reply(window, cx);
            })
        });
        draw(cx);
        draw(cx);
        let refusal = panel
            .read_with(cx, |panel, _| panel.error.clone())
            .expect("an empty reply is refused");
        assert!(
            cx.debug_bounds("github-reply-error").is_some(),
            "the refusal answers in the first conversation's composer"
        );
        let (heard, text) = alerted(cx).expect("an alert");
        assert_eq!(text, refusal, "the window says the refusal");

        // Reply on the second.
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.begin_reply(second.clone(), window, cx)
            })
        });
        draw(cx);
        draw(cx);
        assert_eq!(
            panel.read_with(cx, |panel, _| panel.conversations.active.clone()),
            Some(second.clone()),
            "the second conversation's composer is open"
        );
        assert!(
            cx.debug_bounds("github-reply-error").is_none()
                && cx.debug_bounds("github-reply-cancelled").is_none(),
            "nothing answers under the second conversation's draft"
        );
        assert!(
            cx.debug_bounds("github-error").is_some(),
            "the first conversation's refusal stands on the panel's top line"
        );
        assert_eq!(
            alerted(cx).map(|(id, _)| id),
            Some(heard),
            "and is said once"
        );
        escape(cx);
    }

    /// A Files row whose preparation fails says so on the panel's top line, though the last
    /// outcome before it, a refused Review reply…, answered in the open reply's composer: no
    /// request carried the preparation, so it takes none's origin. It once kept that origin
    /// and stood under the reply's draft.
    #[gpui::test]
    async fn a_file_preparation_failure_answers_on_the_top_line(cx: &mut TestAppContext) {
        use crate::tags::tests::draw;
        let (_fixture, app, panel, cx) = notice_panel(cx);
        let pull = github::review::fixture_pull(false);
        let identity = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let page = Client(UiTransport::Fixture { moved: false })
            .review_threads(&identity, None, &OperationControl::default())
            .unwrap();
        let target = page.items[0].target.clone();
        cx.update(|_, cx| {
            panel.update(cx, |panel, _| {
                panel.pull = Some(pull);
                panel.review.show_list = false;
                panel.review.section = Section::Files;
                panel.receive_threads(page, None);
                // GitHub returned a path the panel refuses to prepare.
                panel.review.files = vec![github::review::PullFile {
                    filename: "../outside".into(),
                    previous_filename: None,
                    status: "modified".into(),
                    additions: 1,
                    deletions: 0,
                    patch: Some("@@ -0,0 +1 @@\n+line\n".into()),
                    rows: Vec::new(),
                    unavailable: None,
                }];
            })
        });
        open_notice_panel(&app, &panel, (13, 1480., 980.), cx).await;

        // Review reply… with nothing written: refused in the reply's composer.
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.begin_reply(target.clone(), window, cx);
                panel.review_reply(window, cx);
            })
        });
        draw(cx);
        draw(cx);
        assert!(
            cx.debug_bounds("github-reply-error").is_some(),
            "the refusal answers in the reply's composer"
        );

        // The file, chosen, fails to prepare off the UI thread.
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| panel.select_review_file(0, window, cx))
        });
        cx.run_until_parked();
        draw(cx);
        draw(cx);
        assert_eq!(
            panel
                .read_with(cx, |panel, _| panel.error.clone())
                .as_deref(),
            Some("GitHub returned an invalid changed-file path"),
            "the preparation failed"
        );
        assert!(
            cx.debug_bounds("github-error").is_some(),
            "its failure stands on the panel's top line"
        );
        assert!(
            cx.debug_bounds("github-reply-error").is_none(),
            "nothing answers under the reply's draft"
        );
        escape(cx);
    }

    /// The connection pane's help says what Connect does on this platform, from whether its
    /// store holds GitTurtle's authorization (`SecureStore::HOLDS`): on a Mac it saves it in
    /// macOS Keychain, and on Linux in your desktop keyring, which it never calls Keychain,
    /// encrypted or secure storage; elsewhere, where the store saves none, it names no storage
    /// and says Connect cannot finish there. Linux once said Connect saved it in secure
    /// storage, then that GitTurtle stored no authorization there.
    #[gpui::test]
    async fn the_connection_help_says_what_connect_keeps(cx: &mut TestAppContext) {
        use crate::copy_feedback::tests::click;
        use crate::native_accessibility::written;
        use crate::tags::tests::draw;
        let (_fixture, app, panel, cx) = notice_panel(cx);
        open_notice_panel(&app, &panel, (13, 1480., 980.), cx).await;
        click(cx, "github-account-options");
        draw(cx);
        draw(cx);
        let help = written::get("github-connection-help")
            .map(|line| line.node)
            .and_then(|node| node.label().or(node.value()).map(str::to_owned))
            .expect("the connection pane says what Connect does");
        assert!(
            help.starts_with("Connect uses your current GitHub CLI account"),
            "{help}"
        );
        #[cfg(target_os = "macos")]
        assert!(
            help.contains("saves GitTurtle authorization in macOS Keychain."),
            "Connect saves the authorization where it says: {help}"
        );
        #[cfg(target_os = "linux")]
        {
            assert!(
                help.contains("saves GitTurtle authorization in your desktop keyring."),
                "Connect saves the authorization where it says: {help}"
            );
            assert!(!misnames_the_linux_store(&help), "{help}");
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            for storage in ["saves", "storage", "Keychain"] {
                assert!(
                    !help.contains(storage),
                    "no storage where nothing is stored ({storage}): {help}"
                );
            }
            assert!(
                help.contains("GitTurtle stores no authorization on this platform"),
                "it says nothing is stored: {help}"
            );
        }
        escape(cx);
    }

    /// While a conversation is open, the panel's top line stands above the body
    /// ([`Panel::pin_top_line`]), and as it appears, changes or clears, nothing drawn in the
    /// conversation moves, in the very frame it changes: a PR's Changes asked for while a reply
    /// is being written, its busy row appearing, then the operation bar's Cancel and the
    /// "Cancelled" line in place of the row, and the line clearing, at 18 pt in a 461 × 490
    /// window, whose body is about 265 px tall, and at 13 pt in a 1,480 × 980 one. Focus stays
    /// in the draft, and the body scrolled to its top shows its first row whole below the
    /// line. Among the content under the toolbar, the line pushed the conversation, the draft
    /// included, down by its height.
    #[gpui::test]
    async fn the_top_line_moves_nothing_in_an_open_conversation(cx: &mut TestAppContext) {
        use crate::views::tests::draw;
        // The conversation's rows, in the order the body draws them, and its draft's composer.
        const ROWS: [&str; 10] = [
            "github-fixture-mode",
            "github-destination",
            "github-pull-title",
            "github-branch-context",
            "github-captured-identities",
            "github-threads",
            "github-open-changes",
            "github-reply-composer",
            "github-reply-context",
            "github-recovery-toggle",
        ];
        let (_fixture, app, panel, cx) = busy_fixture_panel(cx).await;
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
        let pull = local_pull(&app, cx);
        let identity = github::review::fixture_pull(false)
            .capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let page = Client(UiTransport::Fixture { moved: false })
            .review_threads(&identity, None, &OperationControl::default())
            .unwrap();
        let target = page.items[0].target.clone();
        cx.update(|_, cx| {
            cx.set_reduce_motion(true);
            panel.update(cx, |panel, _| {
                panel.pull = Some(pull);
                panel.review.show_list = false;
                panel.review.section = Section::Overview;
                panel.receive_threads(page, None);
            })
        });
        let scroll = panel.read_with(cx, |panel, _| panel.review.body_scroll.clone());
        // Where the body places its content: its top with its scroll, which the rows lie below
        // by their places in the content.
        let placed = |scroll: &ScrollHandle| scroll.bounds().top() + scroll.offset().y;
        // `change`, then the frame that draws it, in one update, and where that frame placed
        // the body's content, read before anything the frame deferred runs. A test window draws
        // itself as an update ends, and again after what that frame deferred, so a scroll moved
        // a frame late, after a frame drawn without it, would be drawn by the time the frame's
        // bounds could be read.
        let frame =
            |cx: &mut VisualTestContext,
             change: &dyn Fn(&mut Panel, &mut Window, &mut Context<Panel>)| {
                cx.update(|window, cx| {
                    panel.update(cx, |panel, cx| {
                        change(panel, window, cx);
                        cx.notify();
                    });
                    window.simulate_next_frame(cx);
                    window.draw(cx).clear(cx);
                    placed(&scroll)
                })
            };
        let rows = |cx: &mut VisualTestContext| {
            ROWS.map(|row| (row, cx.debug_bounds(row).expect("the row is drawn")))
        };
        let in_draft = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                let input = panel.read(cx).conversations.input.read(cx).focus_handle(cx);
                input.is_focused(window)
            })
        };
        for (interface, width, height) in [(18, 461., 490.), (13, 1480., 980.)] {
            let at = format!("{interface} pt in {width} × {height}");
            // As the request that loaded the PR leaves the line: empty.
            cx.update(|_, cx| {
                panel.update(cx, |panel, _| {
                    panel.notice = None;
                    panel.cancelled = None;
                })
            });
            open_notice_panel(&app, &panel, (interface, width, height), cx).await;
            // Reply, written in its draft, with the body scrolled to show the draft from its
            // context line at the body's top, so the scroll has room to move either way.
            cx.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    panel.begin_reply(target.clone(), window, cx);
                    panel
                        .conversations
                        .input
                        .update(cx, |input, cx| input.set_value("Exact reply", window, cx));
                })
            });
            draw(cx);
            let body = cx.debug_bounds("github-panel").expect("the body");
            let context = cx
                .debug_bounds("github-reply-context")
                .expect("the draft's context");
            scroll.set_offset(point(
                px(0.),
                (scroll.offset().y - (context.top() - body.top()))
                    .clamp(-scroll.max_offset().y, px(0.)),
            ));
            draw(cx);
            let body = cx.debug_bounds("github-panel").expect("the body");
            let context = cx
                .debug_bounds("github-reply-context")
                .expect("the draft's context");
            assert!(
                context.top() >= body.top() && context.bottom() <= body.bottom(),
                "{at}: the draft's context {context:?} is in view in {body:?}"
            );
            assert!(
                cx.debug_bounds("github-top-line").is_none(),
                "{at}: the line has nothing to say"
            );
            assert!(in_draft(cx), "{at}: the draft holds focus");
            let first = ROWS
                .into_iter()
                .find(|row| {
                    cx.debug_bounds(row)
                        .is_some_and(|bounds| bounds.bottom() > body.top())
                })
                .expect("a row is in view");
            let still = rows(cx);
            let content = placed(&scroll);
            let in_frame = |drawn: Pixels, after: &str| {
                assert_eq!(
                    drawn, content,
                    "{at}: {after}, the frame placed the body's content where it was"
                );
            };
            let unmoved = |cx: &mut VisualTestContext, after: &str| {
                assert_eq!(
                    rows(cx),
                    still,
                    "{at}: {after}, nothing in the conversation moved, its first row in view \
                     {first} included"
                );
                assert!(in_draft(cx), "{at}: {after}, focus stays in the draft");
            };

            // Open captured local comparison, held as a large comparison holds it: the line
            // appears with the busy row.
            let held = app.update(cx, |app, _| app.operations.hold());
            let drawn = frame(cx, &|panel, window, cx| panel.changes(window, cx));
            in_frame(drawn, "in the frame the busy row appears");
            assert!(
                cx.debug_bounds("github-busy").is_some(),
                "{at}: the busy row stands on the line"
            );
            unmoved(cx, "in the frame the busy row appears");

            // The operation bar's Cancel, which leaves focus where it is; the "Cancelled"
            // line replaces the busy row as the comparison answers.
            cx.update(|window, cx| app.update(cx, |app, cx| app.cancel_operation(window, cx)));
            drop(held);
            app.read_with(cx, |app, _| app.operations.drain());
            settle_panel(&app, cx).await;
            assert!(
                panel.read_with(cx, |panel, _| !panel.pending && panel.cancelled.is_some()),
                "{at}: Changes was cancelled"
            );
            // The answer lands in a task, and the window draws it as the task's update ends.
            draw(cx);
            let body = cx.debug_bounds("github-panel").expect("the body");
            let cancelled = cx
                .debug_bounds("github-cancelled")
                .unwrap_or_else(|| panic!("{at}: the cancel answers on the panel's top line"));
            assert!(
                cancelled.bottom() <= body.top() && cx.debug_bounds("github-busy").is_none(),
                "{at}: the line {cancelled:?} stands above the body {body:?}"
            );
            unmoved(cx, "in the frame the cancel answers");
            draw(cx);
            unmoved(cx, "in the frames after it");

            // The body's top, under the line, is still the body's.
            let offset = scroll.offset();
            scroll.set_offset(point(px(0.), px(0.)));
            draw(cx);
            let line = cx.debug_bounds("github-top-line").expect("the line");
            let body = cx.debug_bounds("github-panel").expect("the body");
            let top = cx.debug_bounds(ROWS[0]).expect("the body's first row");
            assert!(
                top.top() >= line.bottom() && top.top() >= body.top(),
                "{at}: scrolled to its top, the body shows its first row {top:?} below the \
                 line {line:?}"
            );
            scroll.set_offset(offset);
            draw(cx);
            unmoved(cx, "scrolled back");

            // The line clears, as the next request clears it.
            let drawn = frame(cx, &|panel, _, _| panel.cancelled = None);
            in_frame(drawn, "in the frame the line clears");
            assert!(
                cx.debug_bounds("github-top-line").is_none(),
                "{at}: the line cleared"
            );
            unmoved(cx, "in the frame the line clears");
            cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
            settle_panel(&app, cx).await;
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// A reply closing while the panel's top line stands above the body takes the line back
    /// among the content ([`Panel::unpin_top_line`]), and the conversation drawn below the
    /// line stays where it was in the frame the reply closes, the body scrolled partway: for
    /// a "Cancelled", for the moved-head notice and for a notice long enough that the line
    /// caps it above the body, at 13 and 18 pt in a 461 × 490 window and at 13 pt in a
    /// 1,000 × 680 one, where the body still overflows once the composer has gone (in a
    /// 1,480 × 980 window it fits then, and the body scrolls back to its top, moving the
    /// conversation as any view does whose content ends above its fold). With no
    /// adjustment in that frame, the rows moved by the room the line
    /// takes among the content less the height it stood above the body: the divider's pixel,
    /// or all of a capped line's hidden text.
    #[gpui::test]
    async fn closing_a_reply_keeps_the_conversation_where_it_was(cx: &mut TestAppContext) {
        use crate::views::tests::draw;
        // Rows of the conversation below the line, above the composer that closes.
        const ROWS: [&str; 5] = [
            "github-pull-title",
            "github-branch-context",
            "github-captured-identities",
            "github-threads",
            "github-open-changes",
        ];
        let long = format!("{MOVED} {MOVED} {MOVED} {MOVED}");
        let (_fixture, app, panel, cx) = busy_fixture_panel(cx).await;
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
        let pull = local_pull(&app, cx);
        let identity = github::review::fixture_pull(false)
            .capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let page = Client(UiTransport::Fixture { moved: false })
            .review_threads(&identity, None, &OperationControl::default())
            .unwrap();
        let target = page.items[0].target.clone();
        cx.update(|_, cx| {
            cx.set_reduce_motion(true);
            panel.update(cx, |panel, _| {
                panel.pull = Some(pull);
                panel.review.show_list = false;
                panel.review.section = Section::Overview;
                panel.receive_threads(page, None);
            })
        });
        let scroll = panel.read_with(cx, |panel, _| panel.review.body_scroll.clone());
        let text = panel.read_with(cx, |panel, _| panel.review.top_line_scroll.clone());
        let placed = |scroll: &ScrollHandle| scroll.bounds().top() + scroll.offset().y;
        let rows = |cx: &mut VisualTestContext| {
            ROWS.map(|row| cx.debug_bounds(row).expect("the row is drawn").top())
        };
        let mut capped = false;
        for (interface, width, height) in [(13, 461., 490.), (18, 461., 490.), (13, 1000., 680.)] {
            for (said, notice, cancelled) in [
                ("a cancel", None, RequestKind::Compare.cancelled(false)),
                ("the notice", Some(MOVED.to_owned()), None),
                ("a long notice", Some(long.clone()), None),
            ] {
                let at = format!("{said} at {interface} pt in {width} × {height}");
                cx.update(|_, cx| {
                    panel.update(cx, |panel, _| {
                        panel.conversations.active = Some(target.clone());
                        panel.notice = notice.clone();
                        panel.cancelled = cancelled;
                    })
                });
                open_notice_panel(&app, &panel, (interface, width, height), cx).await;
                assert!(
                    cx.debug_bounds("github-top-line").is_some(),
                    "{at}: the line stands above the body beside the open reply"
                );
                capped |= text.max_offset().y > px(0.);
                // The body scrolled partway: the PR's identities at its top.
                let body = cx.debug_bounds("github-panel").expect("the body");
                let identities = cx
                    .debug_bounds("github-captured-identities")
                    .expect("the identities");
                scroll.set_offset(point(
                    px(0.),
                    (scroll.offset().y - (identities.top() - body.top()))
                        .clamp(-scroll.max_offset().y, px(0.)),
                ));
                draw(cx);
                let before = rows(cx);
                let content = placed(&scroll);

                // The reply closes, and the frame that draws it.
                let closed = cx.update(|window, cx| {
                    panel.update(cx, |panel, cx| {
                        panel.conversations.active = None;
                        cx.notify();
                    });
                    window.simulate_next_frame(cx);
                    window.draw(cx).clear(cx);
                    placed(&scroll)
                });
                assert!(
                    cx.debug_bounds("github-top-line").is_none(),
                    "{at}: the line moved back among the content"
                );
                let after = rows(cx);
                let now = placed(&scroll);
                let drawn = after.map(|top| top - now + closed);
                assert_eq!(
                    drawn, before,
                    "{at}: the rows {ROWS:?} stayed where they were as the reply closed \
                     (content placed at {content:?}, then {closed:?})"
                );
                cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
                settle_panel(&app, cx).await;
            }
        }
        assert!(capped, "some layout caps the long notice above the body");
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.conversations.active = None;
                panel.notice = None;
                panel.cancelled = None;
                panel.close(window, cx);
            });
            appearance::apply_text_sizes(13, 12, window, cx)
        });
    }

    /// Reply opened while the panel's top line says something takes the line from among the
    /// content ([`Panel::flow_top_line`]): a "Cancelled" moves above the body
    /// ([`Panel::pin_top_line`]), and a notice from before goes (`Panel::begin_reply`). Either
    /// way the conversation drawn below the line stays where it was in the frame the reply
    /// opens, the body scrolled partway, at 13 pt in a 461 × 490 window and in a 1,480 × 980
    /// one. The panel once took the line above the body for a new line,
    /// though its room among the content went with it, and scrolled the conversation up by
    /// the line's whole height (65 px for the fixture's notice at 461 × 490).
    #[gpui::test]
    async fn opening_a_reply_keeps_the_conversation_where_it_was(cx: &mut TestAppContext) {
        use crate::views::tests::draw;
        // Rows of the conversation below the line, above the composer that opens.
        const ROWS: [&str; 5] = [
            "github-pull-title",
            "github-branch-context",
            "github-captured-identities",
            "github-threads",
            "github-open-changes",
        ];
        let (_fixture, app, panel, cx) = busy_fixture_panel(cx).await;
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
        let pull = local_pull(&app, cx);
        let identity = github::review::fixture_pull(false)
            .capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let page = Client(UiTransport::Fixture { moved: false })
            .review_threads(&identity, None, &OperationControl::default())
            .unwrap();
        let target = page.items[0].target.clone();
        cx.update(|_, cx| {
            cx.set_reduce_motion(true);
            panel.update(cx, |panel, _| {
                panel.pull = Some(pull);
                panel.review.show_list = false;
                panel.review.section = Section::Overview;
                panel.receive_threads(page, None);
            })
        });
        let scroll = panel.read_with(cx, |panel, _| panel.review.body_scroll.clone());
        let placed = |scroll: &ScrollHandle| scroll.bounds().top() + scroll.offset().y;
        let rows = |cx: &mut VisualTestContext| {
            ROWS.map(|row| cx.debug_bounds(row).expect("the row is drawn").top())
        };
        for (interface, width, height) in [(13, 461., 490.), (13, 1480., 980.)] {
            for (said, notice, cancelled) in [
                ("the notice", Some(OFFLINE), None),
                ("a cancel", None, RequestKind::Compare.cancelled(false)),
            ] {
                let at = format!("{said} at {interface} pt in {width} × {height}");
                cx.update(|_, cx| {
                    panel.update(cx, |panel, _| {
                        panel.conversations.active = None;
                        panel.notice = notice.map(str::to_owned);
                        panel.cancelled = cancelled;
                        // Reply saves the drafts and says so under the section tabs, which the
                        // status already says here, so only the line changes as it opens.
                        panel.save_status = "Saving local draft…".into();
                    })
                });
                open_notice_panel(&app, &panel, (interface, width, height), cx).await;
                // The body scrolled partway: the line among the content above its top, the
                // PR's identities at it.
                let body = cx.debug_bounds("github-panel").expect("the body");
                let identities = cx
                    .debug_bounds("github-captured-identities")
                    .expect("the identities");
                scroll.set_offset(point(
                    px(0.),
                    (scroll.offset().y - (identities.top() - body.top()))
                        .clamp(-scroll.max_offset().y, px(0.)),
                ));
                draw(cx);
                assert!(
                    panel.read_with(cx, |panel, _| panel.conversations.active.is_none()),
                    "{at}: no reply is open"
                );
                let before = rows(cx);
                let content = placed(&scroll);

                // Reply, and the frame that draws it, read before the composer's reveal,
                // which the frame defers, scrolls on to the draft.
                let opened = cx.update(|window, cx| {
                    panel.update(cx, |panel, cx| {
                        panel.begin_reply(target.clone(), window, cx)
                    });
                    window.simulate_next_frame(cx);
                    window.draw(cx).clear(cx);
                    placed(&scroll)
                });
                // Reply clears a notice from before; a "Cancelled" moves above the body.
                assert_eq!(
                    cx.debug_bounds("github-top-line").is_some(),
                    notice.is_none(),
                    "{at}: the line stands above the body only for the cancel"
                );
                // Each row where the frame that opened the reply drew it: where the content
                // places it, from where that frame placed the content.
                let after = rows(cx);
                let now = placed(&scroll);
                let drawn = after.map(|top| top - now + opened);
                assert_eq!(
                    drawn, before,
                    "{at}: the rows {ROWS:?} stayed where they were as the reply opened \
                     (content placed at {content:?}, then {opened:?})"
                );
                cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
                settle_panel(&app, cx).await;
            }
        }
        cx.update(|window, cx| {
            panel.update(cx, |panel, _| panel.conversations.active = None);
            appearance::apply_text_sizes(13, 12, window, cx)
        });
    }

    /// Reply says why where the person is looking: opening a conversation's composer clears a
    /// notice from before that is not about that conversation, a resolved conversation's here,
    /// and a reply refused there shows its failure within the body, with focus still in the
    /// draft, at 13 pt in a 461 × 490 window. A notice about the conversation Reply opens, its
    /// reply just sent, stays. Pinned above the body for as long as the reply was open, the
    /// stale notice kept room the composer needed, and the failure stood below the body's clip.
    #[gpui::test]
    async fn reply_clears_a_stale_notice_and_shows_its_refusal(cx: &mut TestAppContext) {
        use crate::tags::tests::draw;
        let (_fixture, app, panel, cx) = notice_panel(cx);
        let pull = github::review::fixture_pull(false);
        let identity = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let page = Client(UiTransport::Fixture { moved: false })
            .review_threads(&identity, None, &OperationControl::default())
            .unwrap();
        let open = page
            .items
            .iter()
            .filter(|thread| thread.reply_available())
            .map(|thread| thread.target.clone())
            .collect::<Vec<_>>();
        let [resolved, other, ..] = &open[..] else {
            panic!("two of the fixture's conversations accept a reply");
        };
        cx.update(|_, cx| {
            panel.update(cx, |panel, _| {
                panel.pull = Some(pull);
                panel.review.show_list = false;
                panel.review.section = Section::Overview;
                panel.receive_threads(page, None);
            })
        });
        open_notice_panel(&app, &panel, (13, 461., 490.), cx).await;
        let in_draft = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                let input = panel.read(cx).conversations.input.read(cx).focus_handle(cx);
                input.is_focused(window)
            })
        };

        // One conversation resolved, as Send to GitHub finishes it; then Reply on another.
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                let action = Action::SetThreadResolved {
                    target: resolved.clone(),
                    resolved: true,
                };
                panel.finish_conversation_action(&action, window, cx);
            })
        });
        draw(cx);
        assert!(
            cx.debug_bounds("github-notice").is_some(),
            "the panel says the conversation was resolved"
        );
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| panel.begin_reply(other.clone(), window, cx))
        });
        draw(cx);
        assert!(
            cx.debug_bounds("github-notice").is_none()
                && cx.debug_bounds("github-top-line").is_none(),
            "the resolved conversation's notice went as Reply opened another"
        );

        // Review reply… with nothing written: refused in the composer, shown in the body.
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.review_reply(window, cx)));
        draw(cx);
        draw(cx);
        let body = cx.debug_bounds("github-panel").expect("the body");
        let line = cx
            .debug_bounds("github-reply-error")
            .expect("the refusal answers in the composer");
        assert!(
            body.top() <= line.top() && line.bottom() <= body.bottom(),
            "the refusal {line:?} stands within the body {body:?}"
        );
        assert!(in_draft(cx), "focus stays in the draft");

        // The reply sent, as Send to GitHub finishes it: Reply on the same conversation keeps
        // the notice about it.
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                let action = Action::Reply {
                    target: other.clone(),
                    body: panel.conversations.input.read(cx).value().to_string(),
                };
                panel.finish_conversation_action(&action, window, cx);
                panel.begin_reply(other.clone(), window, cx);
            })
        });
        draw(cx);
        assert!(
            cx.debug_bounds("github-notice").is_some(),
            "the notice about the reply sent on this conversation stays"
        );
        escape(cx);
    }

    /// The top line's text, where it is taller than its room above an open conversation, scrolls
    /// within the line, and the line's scrollbar cues the rest, its thumb in a gutter clear of
    /// the text: the fixture's notice with a long failure at 18 pt in a 461 × 490 window. A
    /// short line keeps the same gutter, the body's right inset, and paints no thumb. The text
    /// once scrolled with no sign of more, a long remedy cut at the line's edge.
    #[gpui::test]
    async fn the_top_lines_long_text_cues_its_scrollbar(cx: &mut TestAppContext) {
        use crate::tags::tests::{draw, painted_thumb};
        use gpui_kit::component::scroll::Scrollbar;
        const FAILURE: &str = "PR changes need their captured head, base, and common ancestor \
                               locally. Use an explicit Fetch if needed, then reopen the PR. \
                               The comparison could not find the captured base revision in \
                               the local repository, nor the captured head revision, nor a \
                               common ancestor of the two.";
        let (_fixture, app, panel, cx) = notice_panel(cx);
        let pull = github::review::fixture_pull(false);
        let identity = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let page = Client(UiTransport::Fixture { moved: false })
            .review_threads(&identity, None, &OperationControl::default())
            .unwrap();
        let target = page.items[0].target.clone();
        cx.update(|_, cx| {
            panel.update(cx, |panel, _| {
                panel.pull = Some(pull);
                panel.review.show_list = false;
                panel.review.section = Section::Overview;
                panel.receive_threads(page, None);
                panel.conversations.active = Some(target);
            })
        });
        let bar = Scrollbar::width();
        for (interface, width, height, failure, cued) in [
            (18, 461., 490., Some(FAILURE), true),
            (13, 1480., 980., None, false),
        ] {
            let at = format!("{interface} pt in {width} × {height}");
            cx.update(|_, cx| {
                panel.update(cx, |panel, _| {
                    // A failure of a request from the panel, which answers on its top line.
                    panel.origin = Origin::Panel;
                    panel.error = failure.map(str::to_owned);
                })
            });
            open_notice_panel(&app, &panel, (interface, width, height), cx).await;
            draw(cx);
            let text = cx
                .debug_bounds("github-top-line-scroll")
                .expect("the line's text");
            let notice = cx.debug_bounds("github-notice").expect("the notice");
            let gutter = crate::tags::ScrollCue::gutter();
            assert!(
                (text.right() - gutter - notice.right()).abs() < px(0.5),
                "{at}: the notice {notice:?} keeps the gutter {gutter:?} in {text:?}"
            );
            // The scrollbar's viewport, which its track fills.
            let track = Bounds::new(
                point(text.right() - bar - px(4.), text.top()),
                size(bar + px(4.), text.size.height),
            );
            let thumb = painted_thumb(cx, track);
            assert_eq!(
                thumb.is_some(),
                cued,
                "{at}: a scrollbar ({thumb:?}) only where the line's text is taller than its room"
            );
            escape(cx);
        }
        cx.update(|_, cx| panel.update(cx, |panel, _| panel.error = None));
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Send to GitHub starting from a reply's confirmation shows nothing new in the composer,
    /// so the body does not scroll to its draft: the conversation stays where the person
    /// pressed, the draft partly above the body's top, at 13 pt in a 461 × 490 window. A
    /// request starting from the composer clears its last outcome, and that once counted as an
    /// outcome to reveal, scrolling the body back to the draft as Send started.
    #[gpui::test]
    async fn send_from_a_reply_scrolls_nothing_as_it_starts(cx: &mut TestAppContext) {
        use crate::views::tests::draw;
        let (_fixture, app, panel, cx) = busy_fixture_panel(cx).await;
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
        let pull = github::review::fixture_pull(false);
        let identity = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let page = Client(UiTransport::Fixture { moved: false })
            .review_threads(&identity, None, &OperationControl::default())
            .unwrap();
        let target = page.items[0].target.clone();
        cx.update(|_, cx| {
            cx.set_reduce_motion(true);
            panel.update(cx, |panel, _| {
                panel.pull = Some(pull);
                panel.review.show_list = false;
                panel.review.section = Section::Overview;
                panel.receive_threads(page, None);
                // Earlier attempts under the composer, so the body scrolls on past it.
                panel.review.recovery_open = true;
                panel.attempts = (0..10)
                    .map(|index| drafts::Attempt {
                        destination: format!("github.com/gitturtle-fixture/native-review #{index}"),
                        outcome: "Sent".into(),
                        completed_reply_sha256: None,
                    })
                    .collect();
            })
        });
        open_notice_panel(&app, &panel, (13, 461., 490.), cx).await;
        // Reply, written, then Review reply…, which opens its confirmation.
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.begin_reply(target.clone(), window, cx);
                panel
                    .conversations
                    .input
                    .update(cx, |input, cx| input.set_value("Exact reply", window, cx));
                panel.review_reply(window, cx);
            })
        });
        draw(cx);
        assert!(
            panel.read_with(cx, |panel, _| panel.confirm.is_some()),
            "the reply is ready to send"
        );
        // The draft partly above the body's top, as the confirmation's reveal leaves it.
        let scroll = panel.read_with(cx, |panel, _| panel.review.body_scroll.clone());
        let body = cx.debug_bounds("github-panel").expect("the body");
        let context = cx
            .debug_bounds("github-reply-context")
            .expect("the draft's context");
        scroll.set_offset(point(
            px(0.),
            (scroll.offset().y - (context.bottom() + px(30.) - body.top()))
                .clamp(-scroll.max_offset().y, px(0.)),
        ));
        draw(cx);
        let body = cx.debug_bounds("github-panel").expect("the body");
        let composer = cx
            .debug_bounds("github-reply-composer")
            .expect("the composer");
        assert!(
            composer.top() < body.top() && composer.bottom() > body.top(),
            "the draft {composer:?} lies partly above the body {body:?}"
        );

        // Send to GitHub, held as a slow request is.
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.submit(window, cx)));
        draw(cx);
        draw(cx);
        assert!(
            panel.read_with(cx, |panel, _| panel.pending),
            "Send is under way"
        );
        assert_eq!(
            cx.debug_bounds("github-reply-composer"),
            Some(composer),
            "the composer stays where it was as Send starts"
        );
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        settle_panel(&app, cx).await;
        cx.update(|_, cx| {
            panel.update(cx, |panel, _| {
                panel.conversations.active = None;
                panel.attempts.clear();
                panel.review.recovery_open = false;
            })
        });
    }

    /// A reply refused in its composer shows its failure within the body, though the body was
    /// scrolled to leave the draft's bottom at its edge, the failure's place below the clip,
    /// at 18 pt in a 461 × 490 window with no notice above the body: the body scrolls it into
    /// view with the draft, and focus stays in the draft. Scrolled away after, the body stays
    /// where the wheel left it. With nothing to show the failure, it stood below the clip.
    #[gpui::test]
    async fn a_refused_reply_scrolls_into_view_below_the_draft(cx: &mut TestAppContext) {
        use crate::tags::tests::draw;
        let (_fixture, app, panel, cx) = notice_panel(cx);
        let pull = github::review::fixture_pull(false);
        let identity = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let page = Client(UiTransport::Fixture { moved: false })
            .review_threads(&identity, None, &OperationControl::default())
            .unwrap();
        let target = page.items[0].target.clone();
        cx.update(|_, cx| {
            panel.update(cx, |panel, _| {
                panel.pull = Some(pull);
                panel.review.show_list = false;
                panel.review.section = Section::Overview;
                panel.receive_threads(page, None);
                panel.notice = None;
            })
        });
        open_notice_panel(&app, &panel, (18, 461., 490.), cx).await;
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.begin_reply(target.clone(), window, cx)
            })
        });
        draw(cx);
        let wheel = |cx: &mut VisualTestContext, by: Pixels| {
            let body = cx.debug_bounds("github-panel").expect("the body");
            cx.simulate_event(ScrollWheelEvent {
                position: body.center(),
                delta: ScrollDelta::Pixels(point(px(0.), by)),
                modifiers: Modifiers::default(),
                touch_phase: TouchPhase::Moved,
            });
            draw(cx);
        };
        // The draft's bottom at the body's bottom edge.
        let body = cx.debug_bounds("github-panel").expect("the body");
        let draft = cx.debug_bounds("github-reply-draft").expect("the draft");
        wheel(cx, body.bottom() - draft.bottom());
        let draft = cx.debug_bounds("github-reply-draft").expect("the draft");
        assert!(
            (draft.bottom() - body.bottom()).abs() < px(1.),
            "the draft {draft:?} ends at the body's edge {body:?}"
        );

        // Review reply… with nothing written: refused under the draft.
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.review_reply(window, cx)));
        draw(cx);
        draw(cx);
        let body = cx.debug_bounds("github-panel").expect("the body");
        let line = cx
            .debug_bounds("github-reply-error")
            .expect("the refusal answers in the composer");
        assert!(
            body.top() <= line.top() && line.bottom() <= body.bottom(),
            "the refusal {line:?} stands within the body {body:?}"
        );
        assert!(
            cx.update(|window, cx| {
                let input = panel.read(cx).conversations.input.read(cx).focus_handle(cx);
                input.is_focused(window)
            }),
            "focus stays in the draft"
        );

        // Scrolled away, the body stays where the wheel leaves it.
        let scroll = panel.read_with(cx, |panel, _| panel.review.body_scroll.clone());
        wheel(cx, px(80.));
        let left = scroll.offset();
        draw(cx);
        draw(cx);
        assert_eq!(scroll.offset(), left, "the failure is not shown again");
        escape(cx);
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// A notice wraps the same above an open conversation as among the content: pinned
    /// ([`Panel::pin_top_line`]), the line keeps the body's right inset, the scroll gutter its
    /// content always keeps, so the notice keeps its width and its wrapped lines, at 13 and
    /// 18 pt in a 461 × 490 window and at 13 pt in a 1,480 × 980 one. Pinned at the panel's
    /// full width, it was 19 px wider than among the content, and a long notice could
    /// re-wrap as it moved above the body.
    #[gpui::test]
    async fn a_pinned_notice_wraps_as_it_did_among_the_content(cx: &mut TestAppContext) {
        let (_fixture, app, panel, cx) = notice_panel(cx);
        let pull = github::review::fixture_pull(false);
        let identity = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let page = Client(UiTransport::Fixture { moved: false })
            .review_threads(&identity, None, &OperationControl::default())
            .unwrap();
        let target = page.items[0].target.clone();
        cx.update(|_, cx| {
            panel.update(cx, |panel, _| {
                panel.pull = Some(pull);
                panel.review.show_list = false;
                panel.review.section = Section::Overview;
                panel.receive_threads(page, None);
                panel.notice = Some(MOVED.into());
            })
        });
        for sizes in [(13, 461., 490.), (18, 461., 490.), (13, 1480., 980.)] {
            let mut drawn = Vec::new();
            for active in [None, Some(target.clone())] {
                cx.update(|window, cx| {
                    panel.update(cx, |panel, cx| {
                        panel.close(window, cx);
                        panel.conversations.active = active.clone();
                    })
                });
                open_notice_panel(&app, &panel, sizes, cx).await;
                assert_eq!(
                    cx.debug_bounds("github-top-line").is_some(),
                    active.is_some(),
                    "{sizes:?}: the line stands above the body only beside a conversation"
                );
                let notice = cx.debug_bounds("github-notice").expect("the notice");
                // Counted at the height the notice's text draws its lines, a whole number of
                // them: the root style's line height is the interface size's, not the
                // notice's 12 pt, and rounds to a whole point.
                let lines = notice.size.height / label_line(cx);
                assert!(
                    (lines - lines.round()).abs() < 0.05,
                    "{sizes:?}: the notice {notice:?} is {lines} of its lines tall"
                );
                drawn.push((notice.size.width, lines.round(), notice.size.height));
            }
            assert!(
                drawn[0].1 > 1.,
                "{sizes:?}: the notice wraps among the content: {drawn:?}"
            );
            assert_eq!(
                drawn[1], drawn[0],
                "{sizes:?}: pinned, the notice keeps its width and wrapped lines \
                 (width, lines, height)"
            );
        }
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.conversations.active = None;
                panel.close(window, cx);
            });
            appearance::apply_text_sizes(13, 12, window, cx)
        });
    }

    /// The top line's text, taller than its room above an open conversation and scrolled to
    /// its end, shows a new failure from its first sentence, and the same failure answering a
    /// press again too, at 18 pt in a 461 × 490 window. A new long failure once opened where
    /// the last one had been scrolled to, its start out of view.
    #[gpui::test]
    async fn a_new_top_line_shows_from_its_first_sentence(cx: &mut TestAppContext) {
        use crate::tags::tests::draw;
        const FAILURE: &str = "PR changes need their captured head, base, and common ancestor \
                               locally. Use an explicit Fetch if needed, then reopen the PR. \
                               The comparison could not find the captured base revision in \
                               the local repository, nor the captured head revision.";
        let (_fixture, app, panel, cx) = notice_panel(cx);
        let pull = github::review::fixture_pull(false);
        let identity = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let page = Client(UiTransport::Fixture { moved: false })
            .review_threads(&identity, None, &OperationControl::default())
            .unwrap();
        let target = page.items[0].target.clone();
        cx.update(|_, cx| {
            panel.update(cx, |panel, _| {
                panel.pull = Some(pull);
                panel.review.show_list = false;
                panel.review.section = Section::Overview;
                panel.receive_threads(page, None);
                panel.conversations.active = Some(target);
                panel.refuse(FAILURE);
            })
        });
        open_notice_panel(&app, &panel, (18, 461., 490.), cx).await;
        let text = panel.read_with(cx, |panel, _| panel.review.top_line_scroll.clone());
        let to_end = |cx: &mut VisualTestContext| {
            assert!(
                text.max_offset().y > px(0.),
                "the line's text is taller than its room"
            );
            text.set_offset(point(px(0.), -text.max_offset().y));
            draw(cx);
            assert!(
                text.offset().y < px(0.),
                "the line's text is scrolled to its end"
            );
        };
        for (press, failure) in [
            (
                "a new failure",
                "The repository window is unavailable. Reopen it and try again.",
            ),
            (
                "the same failure again",
                "The repository window is unavailable. Reopen it and try again.",
            ),
        ] {
            to_end(cx);
            cx.update(|_, cx| {
                panel.update(cx, |panel, cx| {
                    panel.refuse(format!("{failure} {FAILURE}"));
                    cx.notify();
                })
            });
            draw(cx);
            assert_eq!(
                text.offset().y,
                px(0.),
                "{press}: the line's text shows from its first sentence"
            );
        }
        escape(cx);
        cx.update(|_, cx| panel.update(cx, |panel, _| panel.error = None));
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Each press the panel refuses is said again, with a fresh alert node, though it refuses
    /// it as it refused the one before: Review submission… pressed twice with no pull request
    /// chosen. The lines alone stay the same, so the second press was silent.
    #[gpui::test]
    async fn the_window_alerts_each_refused_press(cx: &mut TestAppContext) {
        use crate::copy_feedback::tests::alerted;
        use crate::tags::tests::draw;
        let (_fixture, app, panel, cx) = notice_panel(cx);
        open_notice_panel(&app, &panel, (13, 461., 490.), cx).await;
        #[cfg(target_os = "linux")]
        let mut bus = {
            use crate::native_accessibility::atspi_state_tests::{Bus, next_frame};
            Bus::new(next_frame(cx))
        };
        let refused = DraftIssue::Selection.message();
        let mut heard = Vec::new();
        for press in ["first", "second"] {
            cx.update(|window, cx| {
                panel.update(cx, |panel, cx| panel.review_submission(window, cx))
            });
            draw(cx);
            draw(cx);
            let (id, text) = alerted(cx).unwrap_or_else(|| panic!("the {press} press is said"));
            assert_eq!(text, refused, "the {press} press is refused");
            heard.push(id);
            #[cfg(target_os = "linux")]
            {
                use crate::native_accessibility::atspi_state_tests::next_frame;
                bus.update(next_frame(cx));
                assert_eq!(
                    bus.announced(),
                    vec![refused.to_owned()],
                    "AT-SPI announces the {press} press"
                );
            }
        }
        assert_ne!(heard[0], heard[1], "each press is a fresh alert node");
        escape(cx);
    }

    /// A request that answers while keyboard focus is on the busy row's
    /// Cancel takes the row away and hands focus to the section the panel
    /// shows, before a PR is chosen its destination field, drawn with its
    /// accent edge, and Escape still closes the panel. Focus stayed on the
    /// Cancel no frame drew, where Escape reached nothing.
    #[gpui::test]
    async fn a_request_landing_while_its_cancel_holds_focus_leaves_focus_in_the_panel(
        cx: &mut TestAppContext,
    ) {
        use crate::tags::tests::draw;
        let (_fixture, app, cx) = repository_window(cx);
        cx.simulate_resize(size(px(1480.), px(980.)));
        draw(cx);
        let panel = cx.update(|window, cx| {
            let repo = app.read(cx).repository.clone().unwrap();
            let panel = cx.new(|cx| Panel::new(app.downgrade(), repo, "main".into(), window, cx));
            panel.update(cx, |panel, cx| {
                panel.review.fixture = true;
                panel.destination.update(cx, |input, cx| {
                    input.set_value("gitturtle-fixture/native-review", window, cx)
                });
                panel.close(window, cx);
            });
            app.update(cx, |app, cx| app.open_github(window, cx));
            panel
        });
        settle_panel(&app, cx).await;
        draw(cx);

        // Refresh while the operation thread is held, as by a slow request,
        // with keyboard focus on the busy row's Cancel.
        let held = app.read_with(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.refresh(1, window, cx)));
        draw(cx);
        let cancel = panel.read_with(cx, |panel, _| panel.cancel_request.handle().clone());
        cx.update(|window, cx| window.focus(&cancel, cx));
        draw(cx);
        assert!(cx.debug_bounds("github-cancel").is_some());
        assert!(
            !cx.update(|window, _| window.context_stack().is_empty()),
            "the Cancel holds focus, drawn"
        );

        drop(held);
        settle_panel(&app, cx).await;
        draw(cx);
        assert!(
            cx.debug_bounds("github-cancel").is_none(),
            "the reply takes the busy row away"
        );
        let (destination, text) = cx.read(|cx| {
            let field = panel.read(cx).destination.read(cx);
            (field.focus_handle(cx), field.input_bounds())
        });
        assert!(
            cx.update(|window, _| destination.is_focused(window)),
            "focus moves from the Cancel to the destination field"
        );
        assert!(
            !cx.update(|window, _| window.context_stack().is_empty()),
            "the frame draws the field that holds focus"
        );
        // The field's own accent edge shows its focus whole. The Button's
        // ring outside it is not what this reads: this window installs no
        // palette, so `button_focus_ring` keeps the kit's default, 3 px of
        // `ring` at half opacity with no gap, and the field draws that ring.
        // Its strips are `ring` at half, not the accent the match below asks
        // for exactly, so the field's 1 px edge is the one outline it finds.
        // With a palette installed, the Button's ring in the full accent
        // would match as well.
        let accent = cx.read(|cx| gpui_kit::component::Theme::global(cx).ring);
        let edges: Vec<_> = focus_ring::painted_outlines(cx, accent)
            .into_iter()
            .filter(|edge| edge.bounds.contains(&text.center()))
            .collect();
        assert!(
            matches!(edges[..], [edge] if edge.whole && edge.width == px(1.)),
            "the destination field draws its accent edge whole: {edges:?}"
        );

        escape(cx);
        assert!(
            !cx.update(|window, cx| window.has_active_dialog(cx)),
            "Escape closes the panel"
        );
    }

    /// Each PR row is one text column: the title, a Button's label, and the
    /// context line below the Button start at the Button's label inset and
    /// end at the same edge, whatever the title's length, and each keeps one
    /// line, cut with an ellipsis, in a wide and a narrow window.
    #[gpui::test]
    async fn pull_request_rows_share_one_text_column(cx: &mut TestAppContext) {
        use crate::tags::tests::draw;
        let (_fixture, app, cx) = repository_window(cx);
        let repo = cx.read(|cx| app.read(cx).repository.clone().unwrap());
        let panel = cx.update(|window, cx| {
            cx.new(|cx| Panel::new(app.downgrade(), repo, "main".into(), window, cx))
        });
        app.read_with(cx, |app, _| app.operations.drain());
        cx.run_until_parked();
        let short = github::review::fixture_pull(false);
        let mut long = short.clone();
        long.number = 43;
        long.title = "Keep every captured review position through a repository change ".repeat(6);
        long.head.branch = format!("feature/{}", "a-long-topic-branch-name-".repeat(6));
        long.user.login = "a-reviewer-with-a-long-account-name".into();
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.rows = vec![short, long];
                panel.close(window, cx);
            });
            app.update(cx, |app, cx| app.open_github(window, cx));
        });
        settle_panel(&app, cx).await;
        let inset = appearance::ui_size(crate::LABEL_INSET);
        for width in [1480., 640.] {
            cx.simulate_resize(size(px(width), px(980.)));
            draw(cx);
            let lines = |cx: &mut VisualTestContext, number: u64| {
                let mut bounds = |selector: String| {
                    cx.debug_bounds(selector.clone().leak())
                        .unwrap_or_else(|| panic!("{selector} at {width} px"))
                };
                (
                    bounds(format!("github-pr-{number}")),
                    bounds(format!("github-pr-title-{number}")),
                    bounds(format!("github-pr-context-{number}")),
                )
            };
            let (_, short_title, short_context) = lines(cx, 42);
            for number in [42, 43] {
                let (button, title, context) = lines(cx, number);
                let at = format!("#{number} at {width} px: title {title:?}, context {context:?}");
                assert!(
                    (title.left() - (button.left() + inset)).abs() < px(0.01),
                    "{at}"
                );
                assert!((title.left() - context.left()).abs() < px(0.01), "{at}");
                assert!((title.right() - context.right()).abs() < px(0.01), "{at}");
                assert!(context.right() <= button.right() - inset + px(0.01), "{at}");
                assert_eq!(title.size.height, short_title.size.height, "{at}: one line");
                assert_eq!(
                    context.size.height, short_context.size.height,
                    "{at}: one line"
                );
            }
        }
        cx.update(|window, cx| window.close_dialog(cx));
    }

    /// PR rows are list rows, as the Files list's are: unselected, a row rests on the panel
    /// surface and lifts to the hover surface under both its lines; the selected row rests on
    /// the selected surface and lifts to the selected row's hover. A press on the title or the
    /// row's foot darkens the whole row to the selected surface, as list rows do. The title
    /// Button draws no fill of its own in any of these, so the row lifts and darkens once.
    /// The whole lit row opens the PR as the title does, at 18 pt even 6 px in from its edge
    /// in the row's foot. In Midnight and Daylight. Unselected rows once drew nothing at rest,
    /// the pointer lifted the title's line alone, and a press darkened that line alone.
    #[gpui::test]
    async fn pull_request_rows_rest_and_lift_as_list_rows(cx: &mut TestAppContext) {
        use crate::tags::tests::draw;
        let (_fixture, app, cx) = repository_window(cx);
        // The dialog opens at rest, so the pointer lands where the rows were measured: its
        // open animation moved them 10 to 13 px after they were read, which left the pointer
        // above #43 in some runs.
        cx.update(|_, cx| cx.set_reduce_motion(true));
        let panel = cx.update(|window, cx| {
            let repo = app.read(cx).repository.clone().unwrap();
            let panel = cx.new(|cx| Panel::new(app.downgrade(), repo, "main".into(), window, cx));
            panel.update(cx, |panel, cx| {
                panel.review.fixture = true;
                panel.account = Some("offline-reviewer".into());
                panel.destination.update(cx, |input, cx| {
                    input.set_value("gitturtle-fixture/native-review", window, cx)
                });
                let short = github::review::fixture_pull(false);
                let mut other = short.clone();
                other.number = 43;
                panel.rows = vec![short, other];
                panel.close(window, cx);
            });
            app.update(cx, |app, cx| app.open_github(window, cx));
            panel
        });
        settle_panel(&app, cx).await;
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(1480.), px(980.)));
        draw(cx);
        let fill = |color: u32| gpui_kit::Background::from(gpui_kit::Hsla::from(rgb(color)));
        let painted = |cx: &mut VisualTestContext, selector: String| {
            let bounds = cx
                .debug_bounds(selector.clone().leak())
                .unwrap_or_else(|| panic!("{selector} is drawn"));
            appearance::painted_row(cx, bounds).0
        };
        let hover = |cx: &mut VisualTestContext, selector: &str| {
            let bounds = cx.debug_bounds(selector.to_owned().leak()).unwrap();
            cx.simulate_mouse_move(bounds.center(), None, Modifiers::default());
            draw(cx);
        };
        // In a dark and a light palette, each pass from no PR open.
        for theme in [
            appearance::ThemeChoice::Midnight,
            appearance::ThemeChoice::Daylight,
        ] {
            cx.update(|_, cx| theme.apply(None, cx));
            let p = cx.update(|_, cx| palette(cx));
            panel.update(cx, |panel, _| panel.pull = None);
            cx.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
            draw(cx);
            for number in [42, 43] {
                let card = painted(cx, format!("github-pr-card-{number}"));
                assert_eq!(card, [fill(p.panel)], "#{number} rests on the panel");
                assert!(
                    painted(cx, format!("github-pr-{number}")).is_empty(),
                    "#{number}'s title"
                );
            }
            for line in ["github-pr-43", "github-pr-context-43"] {
                hover(cx, line);
                let card = painted(cx, "github-pr-card-43".into());
                assert_eq!(card, [fill(p.row_hover(false))], "{line} lifts the row");
                assert!(
                    painted(cx, "github-pr-43".into()).is_empty(),
                    "{line}: the row lifts once"
                );
                let other = painted(cx, "github-pr-card-42".into());
                assert_eq!(other, [fill(p.panel)], "{line} leaves #42 at rest");
            }

            // A press on the title darkens the whole row, the title drawing nothing of its own;
            // released off the row, on a control it did not press, it opens nothing.
            let title = cx.debug_bounds("github-pr-43").unwrap();
            cx.simulate_mouse_down(title.center(), MouseButton::Left, Modifiers::default());
            draw(cx);
            let pressed = (
                painted(cx, "github-pr-card-43".into()),
                painted(cx, "github-pr-43".into()),
            );
            assert_eq!(
                pressed,
                (vec![fill(p.selected)], vec![]),
                "a press on #43's title"
            );
            let away = cx.debug_bounds("github-pr-42").unwrap().center();
            cx.simulate_mouse_move(away, Some(MouseButton::Left), Modifiers::default());
            cx.simulate_mouse_up(away, MouseButton::Left, Modifiers::default());
            draw(cx);

            // The whole lit row opens the PR: 6 px in from its left edge, in its foot below the
            // context line, where the line's margin and the row's own padding took no click.
            // Pressed there, the whole row darkens.
            let row = cx.debug_bounds("github-pr-card-42").unwrap();
            let foot = point(row.left() + px(6.), row.bottom() - px(2.));
            cx.simulate_mouse_move(foot, None, Modifiers::default());
            cx.simulate_mouse_down(foot, MouseButton::Left, Modifiers::default());
            draw(cx);
            let pressed = (
                painted(cx, "github-pr-card-42".into()),
                painted(cx, "github-pr-42".into()),
            );
            assert_eq!(
                pressed,
                (vec![fill(p.selected)], vec![]),
                "a press at {foot:?}"
            );
            cx.simulate_mouse_up(foot, MouseButton::Left, Modifiers::default());
            settle_panel(&app, cx).await;
            draw(cx);
            assert_eq!(
                panel.read_with(cx, |panel, _| panel.pull.as_ref().map(|pull| pull.number)),
                Some(42),
                "a click at {foot:?} in {row:?} opens its PR"
            );
            // The open PR's "Pull requests" shows the list above it, #42 selected.
            crate::copy_feedback::tests::click(cx, "github-back-list");
            draw(cx);
            let lifted = fill(p.row_hover(true));
            for line in ["github-pr-context-42", "github-pr-42"] {
                hover(cx, line);
                assert_eq!(painted(cx, "github-pr-card-42".into()), [lifted], "{line}");
                assert!(
                    painted(cx, "github-pr-42".into()).is_empty(),
                    "{line}: the row lifts once"
                );
            }
            hover(cx, "github-pr-context-43");
            assert_eq!(painted(cx, "github-pr-card-42".into()), [fill(p.selected)]);
            assert!(painted(cx, "github-pr-42".into()).is_empty(), "{theme:?}");
        }
        cx.update(|window, cx| {
            window.close_dialog(cx);
            appearance::apply_text_sizes(13, 12, window, cx);
        });
    }

    /// In a 1480 × 980 window, as the native review shows the panel, at 13
    /// and 18 pt, Tab reaches the controls along the panel's sides and each
    /// draws its focus ring whole inside every clip around it (the focus
    /// sweep, `focus_ring::sweep`): the PR list's full-width rows, whose rings
    /// the list cut at its own edges, Recover drafts at the left and Comment
    /// on selection at the right, with the body's scrollbar gutter and
    /// without it, and Source's editor, whose box rings its edge as Changes'
    /// does: the editor draws no edge, and Tab onto it showed no ring.
    #[gpui::test]
    async fn the_panel_keeps_its_edge_controls_rings_inside_its_clips(cx: &mut TestAppContext) {
        use crate::focus_ring::sweep;
        let (_fixture, app, cx) = repository_window(cx);
        sweep::prepare(&app, cx);
        let repo = cx.read(|cx| app.read(cx).repository.clone().unwrap());
        let panel = cx.update(|window, cx| {
            cx.new(|cx| Panel::new(app.downgrade(), repo, "main".into(), window, cx))
        });
        app.read_with(cx, |app, _| app.operations.drain());
        cx.run_until_parked();
        let pull = github::review::fixture_pull(false);
        let mut other = pull.clone();
        other.number = 43;
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.rows = vec![pull.clone(), other];
                panel.review.fixture = true;
                panel.close(window, cx);
            });
            app.update(cx, |app, cx| app.open_github(window, cx));
        });
        settle_panel(&app, cx).await;
        cx.simulate_resize(size(px(1480.), px(980.)));
        let captured = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let files = Client(UiTransport::Fixture { moved: false })
            .files(&captured, 1, &OperationControl::default())
            .unwrap()
            .items;
        let scroll = cx.read(|cx| panel.read(cx).review.body_scroll.clone());
        let mut failures = Vec::new();
        let mut gutters = Vec::new();
        for interface in sweep::SIZES {
            sweep::interface_size(cx, interface);
            for (view, controls) in [
                (
                    "list",
                    &["github-pr-42", "github-pr-43", "github-recovery-toggle"][..],
                ),
                (
                    "Files",
                    &["github-inline-start", "github-recovery-toggle"][..],
                ),
                ("Source", &["github-inline-start"][..]),
            ] {
                cx.update(|window, cx| {
                    panel.update(cx, |panel, cx| {
                        if view != "list" {
                            panel.pull = Some(pull.clone());
                            panel.review.files = files.clone();
                            panel.review.section = Section::Files;
                            panel.review.show_list = false;
                            panel.select_review_file(0, window, cx);
                            panel.review_source(usize::from(view == "Source"), window, cx);
                        } else {
                            panel.pull = None;
                            panel.review.show_list = true;
                        }
                        panel.focus_visible_section(window, cx);
                        cx.notify();
                    })
                });
                settle_panel(&app, cx).await;
                let surface = format!("the GitHub panel's {view}");
                let rings = sweep::walk(&app, cx, &surface, interface, &mut failures);
                gutters.push(scroll.max_offset().y > px(0.));
                // Each control's ring, as the walk saw it, lies outside its edge.
                for &control in controls {
                    let bounds = cx
                        .debug_bounds(control)
                        .unwrap_or_else(|| panic!("{control} in {surface} at {interface} pt"));
                    let ringed = rings.iter().any(|ring| {
                        ring.size.width - bounds.size.width >= px(5.99)
                            && ring.size.height - bounds.size.height >= px(5.99)
                            && (ring.center().x - bounds.center().x).abs() < px(0.5)
                    });
                    assert!(
                        ringed,
                        "Tab rings {control} in {surface} at {interface} pt: {bounds:?} among \
                         {rings:?}"
                    );
                }
                // Source's editor draws no edge of its own; its box rings its edge, over the
                // text, inside its border, wherever the body had scrolled it then.
                if view == "Source" {
                    let source = cx
                        .debug_bounds("github-source-patch")
                        .expect("Source's box");
                    let inside = source.dilate(px(-1.));
                    assert!(
                        rings.iter().any(|ring| {
                            [
                                (ring.left(), inside.left()),
                                (ring.right(), inside.right()),
                                (ring.size.height, inside.size.height),
                            ]
                            .into_iter()
                            .all(|(drawn, edge)| (drawn - edge).abs() <= px(0.5))
                        }),
                        "Tab rings Source's box {source:?} at {interface} pt: {rings:?}"
                    );
                }
            }
        }
        sweep::assert_whole(failures);
        assert!(
            gutters.contains(&true) && gutters.contains(&false),
            "the body overflows in some views and fits in others: {gutters:?}"
        );
    }

    /// The panel's text starts at one edge in a 1480 × 980 window at 13 and
    /// 18 pt: the dialog's title, the fixture banner, a notice and each PR
    /// row's two lines start where the pager's and Recover drafts' Buttons,
    /// at the panel's edge, start their labels. They started at four: the
    /// title at the edge, the banner 6.5 px in, the Buttons' labels 10 px and
    /// the PR rows' text 20.75 px, inside their cards. In a PR's Files, the
    /// file-page bound below the files' pager starts there too, a phrase to a
    /// line inside the file list's column; it started at the edge. The column
    /// beside the file list starts its own text one inset in from its edge,
    /// where the Changes button starts its label: the prompt before a file is
    /// chosen, then the file's path and the range hint, with the patch box at
    /// the edge. The path and hint started at the edge, the prompt 16 px in.
    /// The draft status beside the section tabs ends one inset in from the
    /// panel's right edge, where the Buttons there end their labels.
    #[gpui::test]
    async fn the_panel_starts_its_text_at_one_edge(cx: &mut TestAppContext) {
        use crate::tags::tests::draw;
        let (_fixture, app, cx) = repository_window(cx);
        let repo = cx.read(|cx| app.read(cx).repository.clone().unwrap());
        let panel = cx.update(|window, cx| {
            cx.new(|cx| Panel::new(app.downgrade(), repo, "main".into(), window, cx))
        });
        app.read_with(cx, |app, _| app.operations.drain());
        cx.run_until_parked();
        let pull = github::review::fixture_pull(false);
        let mut other = pull.clone();
        other.number = 43;
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.rows = vec![pull, other];
                panel.review.fixture = true;
                panel.notice = Some("Fixture PRs refreshed.".into());
                panel.close(window, cx);
            });
            app.update(cx, |app, cx| app.open_github(window, cx));
        });
        settle_panel(&app, cx).await;
        cx.simulate_resize(size(px(1480.), px(980.)));
        for interface in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
            draw(cx);
            let at = |cx: &mut VisualTestContext, selector: &'static str| {
                cx.debug_bounds(selector)
                    .unwrap_or_else(|| panic!("{selector} at {interface} pt"))
            };
            let edge = at(cx, "github-panel-content").left();
            let text = edge + text_inset();
            let starts = [
                ("title", at(cx, "github-title").left()),
                ("banner", at(cx, "github-fixture-mode").left()),
                ("notice", at(cx, "github-notice").left()),
                ("PR title", at(cx, "github-pr-title-42").left()),
                ("PR context", at(cx, "github-pr-context-42").left()),
                // A Button at the edge starts its label the inset in.
                ("pager", at(cx, "github-previous").left() + text_inset()),
                (
                    "drafts",
                    at(cx, "github-recovery-toggle").left() + text_inset(),
                ),
            ];
            // Within the device pixel layout puts an edge on: at 18 pt the inset is 13.85 px.
            for (name, start) in starts {
                assert!(
                    (start - text).abs() < px(0.5),
                    "{interface} pt: {name} starts at {start:?}, not {text:?}: {starts:?}"
                );
            }
        }
        // The first PR's Files section, before a file is chosen and then with its first file in
        // Changes.
        let pull = github::review::fixture_pull(false);
        let captured = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let files = Client(UiTransport::Fixture { moved: false })
            .files(&captured, 1, &OperationControl::default())
            .unwrap()
            .items;
        cx.update(|_, cx| {
            panel.update(cx, |panel, cx| {
                panel.pull = Some(pull);
                panel.review.files = files;
                panel.review.section = Section::Files;
                panel.review.show_list = false;
                panel.save_status = "Draft saved locally".into();
                cx.notify();
            })
        });
        settle_panel(&app, cx).await;
        for (chosen, interface) in [(false, 13), (false, 18), (true, 13), (true, 18)] {
            if chosen && interface == 13 {
                cx.update(|window, cx| {
                    panel.update(cx, |panel, cx| panel.select_review_file(0, window, cx))
                });
                settle_panel(&app, cx).await;
            }
            cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
            draw(cx);
            let at = |cx: &mut VisualTestContext, selector: &'static str| {
                cx.debug_bounds(selector)
                    .unwrap_or_else(|| panic!("{selector} at {interface} pt in Files"))
            };
            // The column beside the file list starts its text where the mode buttons at its
            // edge start their labels, and its boxes at that edge.
            let column = at(cx, "github-file-column").left();
            let starts = if chosen {
                vec![
                    ("path", at(cx, "github-file-title").left()),
                    (
                        "Changes",
                        at(cx, "github-file-mode-0").left() + text_inset(),
                    ),
                    ("range hint", at(cx, "github-range-hint").left()),
                    ("patch box", at(cx, "github-patch").left() + text_inset()),
                ]
            } else {
                vec![("choose a file", at(cx, "github-choose-file").left())]
            };
            for (name, start) in &starts {
                assert!(
                    (*start - (column + text_inset())).abs() < px(0.5),
                    "{interface} pt, Files' column at {column:?}: {name} starts at {start:?}: \
                     {starts:?}"
                );
            }
            // The draft status ends where the Buttons at the panel's right edge end their
            // labels.
            let end = at(cx, "github-panel-content").right() - text_inset();
            let saved = at(cx, "github-local-save").right();
            assert!(
                (saved - end).abs() < px(0.5),
                "{interface} pt: the draft status ends at {saved:?}, not {end:?}"
            );
            let text = at(cx, "github-panel-content").left() + text_inset();
            let list = at(cx, "github-file-focus");
            let bound = at(cx, "github-file-bound");
            let starts = [
                ("title", at(cx, "github-title").left()),
                (
                    "pager",
                    at(cx, "github-files-previous").left() + text_inset(),
                ),
                ("file bound", bound.left()),
            ];
            for (name, start) in starts {
                assert!(
                    (start - text).abs() < px(0.5),
                    "{interface} pt, Files: {name} starts at {start:?}, not {text:?}: {starts:?}"
                );
            }
            // Below the file list, across the panel, the bound is one line as tall as the
            // page number's, with its separator: in the list's column it took two.
            let line = at(cx, "github-files-page").size.height;
            assert!(
                bound.top() >= list.bottom() && (bound.size.height - line).abs() < px(0.01),
                "{interface} pt: the file bound {bound:?} is not one line {line:?} below the \
                 file list {list:?}"
            );
            let (first, last) = (at(cx, "github-file-bound-0"), at(cx, "github-file-bound-1"));
            assert!(
                first.top() == last.top() && first.left() == bound.left(),
                "{interface} pt: the bound's phrases {first:?} and {last:?} share no line"
            );
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        cx.update(|window, cx| window.close_dialog(cx));
    }

    /// The local draft status ends a PR's section tabs' last line, centred on it, one line
    /// that ends at the panel's label inset whatever its length, elided in its middle where
    /// the tabs leave less room than it needs, its whole text its accessible name: it never
    /// takes a line of its own, so the tabs and the conversation below them stay where they
    /// were as it appears, changes or clears. At 13 and 18 pt in a 1480 × 980, a 1000 × 700
    /// and a 461 × 490 window, with no status, "Saving local draft…", as Reply says, "Draft
    /// saved locally", the failed save's status and the unsaved draft's longer one: at 461 px
    /// "Saving local draft…" once took a line below the tabs and moved the rows under them
    /// 39 px.
    #[gpui::test]
    async fn the_draft_status_stays_on_the_tabs_line(cx: &mut TestAppContext) {
        use crate::tags::tests::{draw, hovered_tooltips};
        // Every text run the window draws is marked (`width_walk::text_mark`).
        let (_fixture, app, panel, pull, cx) = fixture_panel(cx).await;
        let mark = gpui_kit::Background::from(crate::width_walk::text_mark());
        // Where the status's text is drawn inside its box `shown`.
        let drawn_text = |cx: &mut VisualTestContext, shown: Bounds<Pixels>| {
            cx.update(|window, _| {
                let scale = window.scale_factor();
                let logical = |value: gpui_kit::ScaledPixels| px(value.0 / scale);
                window
                    .painted_quads()
                    .into_iter()
                    .filter(|quad| quad.background == mark)
                    .map(|quad| {
                        Bounds::new(
                            point(logical(quad.bounds.origin.x), logical(quad.bounds.origin.y)),
                            size(
                                logical(quad.bounds.size.width),
                                logical(quad.bounds.size.height),
                            ),
                        )
                    })
                    // The runs on its line, not a taller one overlapping it.
                    .filter(|run| {
                        shown.contains(&run.center())
                            && run.top() >= shown.top() - px(0.5)
                            && run.bottom() <= shown.bottom() + px(0.5)
                    })
                    .reduce(|a, b| a.union(&b))
            })
        };
        cx.update(|_, cx| {
            panel.update(cx, |panel, cx| {
                panel.pull = Some(pull);
                panel.review.section = Section::Overview;
                panel.review.show_list = false;
                cx.notify();
            })
        });
        settle_panel(&app, cx).await;
        let saving = "Saving local draft…";
        let saved = "Draft saved locally";
        let failed = "Draft save failed — keep this window open or copy the text";
        let unsaved = DraftIssue::Selection.save_message();
        // Whether some layout elided a status, hovered one it elided in view, and named one.
        let mut elided = false;
        let mut tipped = false;
        #[cfg_attr(not(target_os = "linux"), allow(unused_mut))]
        let mut named_elided = cfg!(not(target_os = "linux"));
        for (interface, width, height) in [
            (13, 1480., 980.),
            (18, 1480., 980.),
            (13, 1000., 700.),
            (18, 1000., 700.),
            (13, 461., 490.),
            (18, 461., 490.),
        ] {
            cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            let mut placed = None;
            for status in ["", saving, saved, failed, unsaved, ""] {
                cx.update(|_, cx| {
                    panel.update(cx, |panel, cx| {
                        panel.save_status = status.into();
                        cx.notify();
                    })
                });
                draw(cx);
                let at = format!("{interface} pt in {width} × {height}, {status:?}");
                let bounds = |cx: &mut VisualTestContext, selector: &'static str| {
                    cx.debug_bounds(selector)
                        .unwrap_or_else(|| panic!("{at}: {selector}"))
                };
                let edge = bounds(cx, "github-panel-content");
                let row = ["github-section-0", "github-section-1", "github-section-2"]
                    .map(|selector| bounds(cx, selector));
                let below = bounds(cx, "github-captured-identities");
                // The tabs and the conversation stay where no status leaves them in the
                // panel, which the dialog places.
                let drawn = (
                    row.map(|tab| tab.origin - edge.origin),
                    below.origin - edge.origin,
                );
                assert_eq!(
                    *placed.get_or_insert(drawn),
                    drawn,
                    "{at}: the tabs or the rows below them moved"
                );
                if status.is_empty() {
                    continue;
                }
                let shown = bounds(cx, "github-local-save");
                let (line, whole) = cx.update(|window, _| {
                    let size = appearance::ui_text(12.);
                    (
                        window
                            .text_style()
                            .line_height
                            .to_pixels(size.into(), window.rem_size()),
                        crate::views::shaped_width(
                            status,
                            size,
                            window.text_style().font_weight,
                            window,
                        ),
                    )
                });
                // On the last tab's line, centred on it, one line ending where the Buttons at
                // the panel's right edge end their labels, and elided where it is narrower
                // than its text.
                let slack = px(0.5);
                let end = edge.right() - text_inset();
                let last = row[2];
                assert!(
                    shown.left() >= last.right()
                        && (shown.right() - end).abs() < slack
                        && shown.size.height <= line + slack
                        && (shown.center().y - last.center().y).abs() < slack,
                    "{at}: {shown:?} ({whole:?} as one line) beside {last:?}, ending at {end:?}"
                );
                let cut = shown.size.width + slack < whole;
                elided |= cut;
                // The drawn text itself ends there, a whole status's text as wide as it is, an
                // elided one's inside the box and narrower; the box ends in the ellipsis's
                // pixel of slack (`views::ellipsis_middle`) after the text's width rounded up
                // to a whole pixel, so a whole text ends at most 2 px before the inset.
                let body = bounds(cx, "github-panel");
                let shows = shown.bottom() <= body.bottom();
                let text = drawn_text(cx, shown);
                assert_eq!(
                    text.is_some(),
                    shows,
                    "{at}: the status draws its text in {shown:?} where the body {body:?} shows it"
                );
                if let Some(text) = text {
                    assert!(
                        if cut {
                            text.left() >= shown.left() - slack
                                && text.right() <= end + slack
                                && text.size.width < whole
                        } else {
                            text.right() <= end
                                && end - text.right() <= px(2.) + slack
                                && (text.size.width - whole).abs() <= px(1.)
                        },
                        "{at}: the status's text {text:?} ({whole:?} whole) ends at {end:?} in its \
                     box {shown:?}"
                    );
                }
                // Hovering an elided status in view shows its whole text in its tooltip.
                if cut && !tipped && shows {
                    review::SAVE_STATUS_TOOLTIP.with(|shown| shown.borrow_mut().take());
                    assert_eq!(
                        hovered_tooltips(cx, "github-local-save"),
                        1,
                        "{at}: hovering the elided status shows a tooltip"
                    );
                    assert_eq!(
                        review::SAVE_STATUS_TOOLTIP
                            .with(|shown| shown.borrow_mut().take())
                            .as_deref(),
                        Some(status),
                        "{at}: the tooltip shows the status's whole text"
                    );
                    tipped = true;
                }
                // Where the body shows it, an assistive technology reaches its whole text.
                #[cfg(target_os = "linux")]
                if shows {
                    use crate::native_accessibility::atspi_state_tests::{next_frame, reached};
                    assert!(
                        reached(next_frame(cx))
                            .iter()
                            .any(|found| found.name == status),
                        "{at}: the status's whole text is its accessible name"
                    );
                    named_elided |= cut;
                }
            }
        }
        assert!(
            elided && tipped && named_elided,
            "a narrow window elides a long status, its tooltip and its name whole in view"
        );
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        cx.update(|window, cx| window.close_dialog(cx));
    }

    /// A draft status the section tabs' last line leaves less room than its insets and an
    /// ellipsis starts a line of its own below the tabs, drawn there, rather than drawing
    /// nothing beside them: at 18 pt with the long counts "Files · 300+", across window
    /// widths from 400 to 520 px, some width leaves the tabs' last line too little, and at
    /// every width the status is at least that wide, and the rows below the tabs stand where
    /// they stood with no status, so saving moves nothing. The status took whatever room the
    /// line left, down to none.
    #[gpui::test]
    async fn a_draft_status_with_too_little_room_starts_a_line_below_the_tabs(
        cx: &mut TestAppContext,
    ) {
        use crate::tags::tests::draw;
        let (_fixture, app, panel, pull, cx) = fixture_panel(cx).await;
        cx.update(|_, cx| {
            panel.update(cx, |panel, cx| {
                panel.pull = Some(pull);
                panel.review.section = Section::Overview;
                panel.review.show_list = false;
                if let Some(file) = panel.review.files.first().cloned() {
                    panel.review.files = vec![file; 300];
                }
                panel.review.files_next = true;
                panel.save_status = "Draft saved locally".into();
                cx.notify();
            })
        });
        settle_panel(&app, cx).await;
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        let least = text_inset() * 2. + appearance::ui_text(12.);
        let slack = px(0.5);
        let mut below = false;
        let rows = |cx: &mut VisualTestContext| {
            let edge = cx
                .debug_bounds("github-panel-content")
                .expect("the content");
            let row = cx
                .debug_bounds("github-captured-identities")
                .expect("the identities");
            row.origin - edge.origin
        };
        for width in (400..=520).step_by(2) {
            cx.simulate_resize(size(px(width as f32), px(490.)));
            let at = format!("18 pt in {width} × 490");
            // No status, then the saved draft's, which moves nothing below the tabs.
            cx.update(|_, cx| {
                panel.update(cx, |panel, cx| {
                    panel.save_status = SharedString::default();
                    cx.notify();
                })
            });
            draw(cx);
            draw(cx);
            let unsaid = rows(cx);
            cx.update(|_, cx| {
                panel.update(cx, |panel, cx| {
                    panel.save_status = "Draft saved locally".into();
                    cx.notify();
                })
            });
            draw(cx);
            draw(cx);
            assert_eq!(
                rows(cx),
                unsaid,
                "{at}: saving moved the rows below the tabs"
            );
            let tab = cx.debug_bounds("github-section-1").expect("the Files tab");
            let last = cx.debug_bounds("github-section-2").expect("the Review tab");
            assert!(
                tab.size.width > px(0.),
                "{at}: the Files tab reads its long count"
            );
            let status = cx
                .debug_bounds("github-local-save")
                .unwrap_or_else(|| panic!("{at}: the status is drawn"));
            assert!(
                status.size.width + text_inset() * 2. >= least - slack,
                "{at}: the status {status:?} keeps at least its insets and an ellipsis"
            );
            below |= status.top() >= last.bottom() - slack;
        }
        assert!(
            below,
            "some width leaves the tabs' last line too little, and the status starts its own"
        );
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        cx.update(|window, cx| window.close_dialog(cx));
    }

    /// Closing the panel while a reply stands its top line above the body, and opening it
    /// again with no reply open, moves nothing in the body as it opens: the closed panel
    /// forgets the line it pinned ([`Panel::close`]), so the reopened panel's first frame
    /// does not take the line among its content for one leaving above the body and scroll
    /// the body by the difference.
    #[gpui::test]
    async fn reopening_after_a_reply_moves_nothing_for_its_line(cx: &mut TestAppContext) {
        use crate::views::tests::draw;
        let long = format!("{MOVED} {MOVED} {MOVED} {MOVED}");
        let (_fixture, app, panel, cx) = busy_fixture_panel(cx).await;
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
        let pull = local_pull(&app, cx);
        let identity = github::review::fixture_pull(false)
            .capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let page = Client(UiTransport::Fixture { moved: false })
            .review_threads(&identity, None, &OperationControl::default())
            .unwrap();
        let target = page.items[0].target.clone();
        cx.update(|_, cx| {
            cx.set_reduce_motion(true);
            panel.update(cx, |panel, _| {
                panel.pull = Some(pull);
                panel.review.show_list = false;
                panel.review.section = Section::Overview;
                panel.receive_threads(page, None);
                panel.conversations.active = Some(target.clone());
                panel.notice = Some(long.clone());
            })
        });
        open_notice_panel(&app, &panel, (13, 461., 490.), cx).await;
        assert!(
            cx.debug_bounds("github-top-line").is_some(),
            "the line stands above the body beside the open reply"
        );
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
        let scroll = panel.read_with(cx, |panel, _| panel.review.body_scroll.clone());
        let offset = cx.update(|window, cx| {
            panel.update(cx, |panel, _| panel.conversations.active = None);
            app.update(cx, |app, cx| app.open_github(window, cx));
            let offset = point(px(0.), -px(40.));
            scroll.set_offset(offset);
            offset
        });
        draw(cx);
        assert!(
            cx.debug_bounds("github-top-line").is_none(),
            "the line stands among the content"
        );
        assert!(scroll.max_offset().y > px(40.), "the body scrolls");
        assert_eq!(
            scroll.offset(),
            offset,
            "the reopened panel's first frame moves the body for no line"
        );
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.notice = None;
                panel.close(window, cx);
            })
        });
    }

    /// Focus on a control the person scrolled out of view stays out of view as a reply
    /// closes and its top line moves back among the content: the body's reveal finds the
    /// viewport it recorded, moved by the body's own change (`shift_body`), and shows no focus
    /// anew. Moved by the scroll's shift instead, the recorded viewport was a body of another
    /// height, and the closing reply scrolled the body back to the destination field.
    #[gpui::test]
    async fn closing_a_reply_keeps_a_focused_control_where_it_was_scrolled(
        cx: &mut TestAppContext,
    ) {
        use crate::views::tests::draw;
        let long = format!("{MOVED} {MOVED} {MOVED} {MOVED}");
        let (_fixture, app, panel, cx) = busy_fixture_panel(cx).await;
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
        let pull = local_pull(&app, cx);
        let identity = github::review::fixture_pull(false)
            .capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let page = Client(UiTransport::Fixture { moved: false })
            .review_threads(&identity, None, &OperationControl::default())
            .unwrap();
        let target = page.items[0].target.clone();
        cx.update(|_, cx| {
            cx.set_reduce_motion(true);
            panel.update(cx, |panel, _| {
                panel.pull = Some(pull);
                panel.review.show_list = false;
                panel.review.section = Section::Overview;
                panel.receive_threads(page, None);
                panel.conversations.active = Some(target.clone());
                panel.notice = Some(long.clone());
            })
        });
        open_notice_panel(&app, &panel, (13, 461., 490.), cx).await;
        let scroll = panel.read_with(cx, |panel, _| panel.review.body_scroll.clone());
        // Focus on the destination field, shown, then the body scrolled to its end, the field
        // above it.
        cx.update(|window, cx| {
            let focus = panel.read(cx).destination.read(cx).focus_handle(cx);
            window.focus(&focus, cx);
        });
        draw(cx);
        draw(cx);
        scroll.set_offset(point(px(0.), -scroll.max_offset().y));
        draw(cx);
        draw(cx);
        let body = cx.debug_bounds("github-panel").expect("the body");
        let field = cx.debug_bounds("github-destination").expect("the field");
        assert!(
            field.bottom() <= body.top(),
            "the field {field:?} lies above the body {body:?}"
        );
        assert!(cx.debug_bounds("github-top-line").is_some());
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.conversations.active = None;
                cx.notify();
            });
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        });
        draw(cx);
        draw(cx);
        assert!(
            cx.debug_bounds("github-top-line").is_none(),
            "the line moved back among the content"
        );
        let body = cx.debug_bounds("github-panel").expect("the body");
        let field = cx.debug_bounds("github-destination").expect("the field");
        assert!(
            field.bottom() <= body.top(),
            "the field {field:?} stays above the body {body:?} where it was scrolled"
        );
        cx.update(|window, cx| {
            assert!(
                panel
                    .read(cx)
                    .destination
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window),
                "focus stays on the field"
            );
            panel.update(cx, |panel, cx| {
                panel.notice = None;
                panel.close(window, cx);
            })
        });
    }

    /// Review's draft status gives way to Review submission…: where it fits beside the
    /// button whole it stays on its line, from
    /// the panel's text inset; where it does not, it takes a line of its own below the
    /// button, wrapping inside the panel's text column, and the button keeps the panel's
    /// right edge. At 18 pt in a 1280 × 800 window the unsaved draft's status pushed the
    /// button past the panel's right edge, where Tab focused it with nothing drawn. At 13
    /// and 18 pt in a 1480 × 980 and a 1000 × 700 window, with "Draft saved locally", the
    /// failed save's status and the unsaved draft's longer one.
    #[gpui::test]
    async fn the_draft_status_gives_way_to_review_submission(cx: &mut TestAppContext) {
        use crate::tags::tests::draw;
        let (_fixture, app, cx) = repository_window(cx);
        let repo = cx.read(|cx| app.read(cx).repository.clone().unwrap());
        let panel = cx.update(|window, cx| {
            cx.new(|cx| Panel::new(app.downgrade(), repo, "main".into(), window, cx))
        });
        app.read_with(cx, |app, _| app.operations.drain());
        cx.run_until_parked();
        let pull = github::review::fixture_pull(false);
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.rows = vec![pull.clone()];
                panel.review.fixture = true;
                panel.close(window, cx);
            });
            app.update(cx, |app, cx| app.open_github(window, cx));
        });
        settle_panel(&app, cx).await;
        cx.update(|_, cx| {
            panel.update(cx, |panel, cx| {
                panel.pull = Some(pull);
                panel.review.section = Section::Review;
                panel.review.show_list = false;
                cx.notify();
            })
        });
        settle_panel(&app, cx).await;
        let saved = "Draft saved locally";
        let failed = "Draft save failed — keep this window open or copy the text";
        let unsaved = DraftIssue::Selection.save_message();
        let mut wrapped = false;
        for (interface, width, height) in [
            (13, 1480., 980.),
            (18, 1480., 980.),
            (13, 1000., 700.),
            (18, 1000., 700.),
        ] {
            cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            for status in [saved, failed, unsaved] {
                cx.update(|_, cx| {
                    panel.update(cx, |panel, cx| {
                        panel.save_status = status.into();
                        cx.notify();
                    })
                });
                draw(cx);
                let at = format!("{interface} pt in {width} × {height}, {status:?}");
                let bounds = |cx: &mut VisualTestContext, selector: &'static str| {
                    cx.debug_bounds(selector)
                        .unwrap_or_else(|| panic!("{at}: {selector}"))
                };
                let edge = bounds(cx, "github-panel-content");
                let submit = bounds(cx, "github-review-submit");
                let shown = bounds(cx, "github-draft-status");
                let (line, whole) = cx.update(|window, _| {
                    let size = appearance::ui_text(12.);
                    (
                        window
                            .text_style()
                            .line_height
                            .to_pixels(size.into(), window.rem_size()),
                        crate::views::shaped_width(
                            status,
                            size,
                            window.text_style().font_weight,
                            window,
                        ),
                    )
                });
                let slack = px(0.5);
                let (start, end) = (edge.left() + text_inset(), edge.right() - text_inset());
                // The button keeps the panel's right edge.
                assert!(
                    (submit.right() - edge.right()).abs() < slack,
                    "{at}: Review submission… {submit:?} ends at the panel's edge {edge:?}"
                );
                assert!(
                    (shown.left() - start).abs() < slack && shown.right() <= end + slack,
                    "{at}: the status {shown:?} lies from {start:?} to at most {end:?}"
                );
                let lines = (shown.size.height / line).round();
                if shown.top() < submit.bottom() {
                    // Beside the button, whole on one line centred on it.
                    assert!(
                        shown.right() <= submit.left()
                            && lines == 1.
                            && shown.size.width + slack >= whole
                            && (shown.center().y - submit.center().y).abs() < slack,
                        "{at}: {shown:?} ({whole:?} on one line) beside {submit:?}"
                    );
                } else {
                    // Below it, its lines holding the whole text.
                    wrapped = true;
                    assert!(
                        status != saved
                            && (shown.size.height - line * lines).abs() < slack
                            && shown.size.width * lines + slack >= whole,
                        "{at}: {shown:?} ({whole:?}) below {submit:?} in {lines} lines of \
                         {line:?}"
                    );
                }
            }
        }
        assert!(wrapped, "a long status takes a line of its own somewhere");
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        cx.update(|window, cx| window.close_dialog(cx));
    }

    /// A PR's Overview keeps its rows inside the panel at 13 and 18 pt in a 461 and a
    /// 1,480 px window, below the body's fold too, where the width walk sees nothing drawn
    /// (`width_walk`): a row that cannot hold its label beside its Button wraps the label
    /// below it, at the panel's text inset. The PR title beside Pull requests, the PR
    /// pager's page and the drafts policy beside Recover drafts ran past a narrow panel on
    /// one line, as the Conversations heading did past its pane at 18 pt.
    #[gpui::test]
    async fn a_prs_overview_wraps_its_rows_inside_a_narrow_panel(cx: &mut TestAppContext) {
        use crate::width_walk::{Page, frame, open};
        let (_fixture, _app, cx, _) = open(Page::GitHub, cx).await;
        let mut wrapped = Vec::new();
        for (interface, width) in [(13, 461.), (18, 461.), (13, 1480.), (18, 1480.)] {
            cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
            cx.simulate_resize(size(px(width), px(980.)));
            for _ in 0..3 {
                frame(cx);
            }
            let at = format!("{interface} pt in {width} px");
            let bounds = |cx: &mut VisualTestContext, selector: &'static str| {
                cx.debug_bounds(selector)
                    .unwrap_or_else(|| panic!("{at}: {selector}"))
            };
            let edge = bounds(cx, "github-panel-content");
            let slack = px(0.5);
            for (button, text) in [
                ("github-back-list", "github-pull-title"),
                ("github-previous", "github-page"),
                ("github-recovery-toggle", "github-passive-policy"),
            ] {
                let (button, text) = (bounds(cx, button), bounds(cx, text));
                assert!(
                    text.right() <= edge.right() + slack && button.right() <= edge.right() + slack,
                    "{at}: {button:?} and {text:?} lie inside the panel {edge:?}"
                );
                if text.top() >= button.bottom() {
                    // Below its Button, at the panel's text inset.
                    wrapped.push(at.clone());
                    assert!(
                        (text.left() - (edge.left() + text_inset())).abs() < slack,
                        "{at}: {text:?} below {button:?} starts at the panel's text inset"
                    );
                } else {
                    // Beside it, where the Button's gap puts it.
                    assert!(
                        text.left() >= button.right(),
                        "{at}: {text:?} beside {button:?}"
                    );
                }
            }
            let pane = bounds(cx, "github-thread-section");
            let heading = bounds(cx, "github-conversations-heading");
            assert!(
                heading.right() <= pane.right() + slack,
                "{at}: the Conversations heading {heading:?} lies inside its pane {pane:?}"
            );
        }
        assert!(
            !wrapped.is_empty(),
            "a narrow panel wraps a label below its Button"
        );
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The panel's body cues what lies below its fold with the shared always-visible
    /// scrollbar (`tags::ScrollCue`) only while it overflows: a PR's Files in a 1000 × 680
    /// window overflow at 13 and 18 pt, and the thumb follows the body's offset in a gutter
    /// clear of the rings of the controls at its right. The PR list with none loaded in a
    /// 1480 × 1200 window fits: the body draws no scrollbar. Either way the content keeps
    /// that gutter, the track and the ring's room, so nothing it holds moves as it comes to
    /// overflow or to fit.
    #[gpui::test]
    async fn the_panel_cues_its_fold_only_while_its_body_overflows(cx: &mut TestAppContext) {
        use crate::tags::tests::{assert_scroll_cue, draw, set_interface_text_size};
        let (_fixture, app, cx) = repository_window(cx);
        // The dialog opens at rest, so the thumb is read where the track was measured.
        cx.update(|_, cx| {
            appearance::ThemeChoice::Midnight.apply(None, cx);
            cx.set_reduce_motion(true);
        });
        let repo = cx.read(|cx| app.read(cx).repository.clone().unwrap());
        let panel = cx.update(|window, cx| {
            cx.new(|cx| Panel::new(app.downgrade(), repo, "main".into(), window, cx))
        });
        app.read_with(cx, |app, _| app.operations.drain());
        cx.run_until_parked();
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| panel.close(window, cx));
            app.update(cx, |app, cx| app.open_github(window, cx));
        });
        settle_panel(&app, cx).await;
        let room = cx.update(|_, cx| appearance::button_ring_room(cx));
        let scroll = cx.read(|cx| panel.read(cx).review.body_scroll.clone());
        cx.simulate_resize(size(px(1480.), px(1200.)));
        draw(cx);
        // The content keeps the ring's room on its left and the gutter on its right.
        let gutter = crate::tags::ScrollCue::gutter();
        let keeps_gutter = |cx: &mut VisualTestContext| {
            let (body, content) = (
                cx.debug_bounds("github-panel").expect("the body"),
                cx.debug_bounds("github-panel-content")
                    .expect("its content"),
            );
            (body.right() - room - gutter - content.right()).abs() < px(0.01)
                && (content.left() - (body.left() + room)).abs() < px(0.01)
        };
        for interface in [13, 18] {
            set_interface_text_size(cx, &app, interface);
            assert!(
                scroll.max_offset().y == px(0.)
                    && cx.debug_bounds("github-panel-scrollbar").is_none(),
                "at {interface} pt the PR list fits the body, which draws no scrollbar"
            );
            assert!(
                keeps_gutter(cx),
                "at {interface} pt the fitting body's content keeps the gutter"
            );
        }
        set_interface_text_size(cx, &app, appearance::DEFAULT_INTERFACE_TEXT_SIZE);

        let pull = github::review::fixture_pull(false);
        let captured = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let files = Client(UiTransport::Fixture { moved: false })
            .files(&captured, 1, &OperationControl::default())
            .unwrap()
            .items;
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.rows = vec![pull.clone()];
                panel.pull = Some(pull);
                panel.review.files = files;
                panel.review.section = Section::Files;
                panel.review.show_list = false;
                panel.select_review_file(0, window, cx);
            })
        });
        settle_panel(&app, cx).await;
        cx.simulate_resize(size(px(1000.), px(680.)));
        draw(cx);
        let controls = [
            "github-panel-content",
            "github-inline-start",
            "github-account-options",
        ];
        assert_scroll_cue(cx, &app, &scroll, "github-panel", room, &controls);
        // Overflowing, the content keeps the same gutter, the track now drawn in it, and the
        // ring's room on its left.
        assert!(
            keeps_gutter(cx),
            "the overflowing body's content keeps the ring's room and the gutter {gutter:?}"
        );
        cx.update(|window, cx| window.close_dialog(cx));
    }

    /// The sizes the reveal tests walk: interface size and window, a half tile at the default
    /// and the largest size and a wide window.
    const REVEAL_SIZES: [(u8, f32, f32); 4] = [
        (13, 461., 490.),
        (18, 461., 490.),
        (13, 1480., 800.),
        (18, 1480., 800.),
    ];

    /// A window whose root marks every text run (`width_walk::text_mark`), with the app's key
    /// bindings, Midnight and dialogs at rest (`focus_ring::sweep::prepare`), and the panel
    /// open on the offline fixture: the destination named, its first PR listed, signed in as
    /// the fixture's reviewer, with the first page of that PR's conversations, as the native
    /// review loads it. Keep the returned directory until the test ends.
    async fn fixture_panel(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        Entity<GitTurtle>,
        Entity<Panel>,
        PullRequest,
        &mut VisualTestContext,
    ) {
        let (fixture, app, cx) = crate::views::tests::styled_history_window(
            cx,
            b"commit refs/heads/main\ncommitter Fixture <fixture@example.invalid> 1700000000 +0000\ndata 13\nfixture notes\nM 100644 inline notes.txt\ndata 6\nnotes\n\ndone\n",
            |root| root.text_bg(crate::width_walk::text_mark()),
        )
        .await;
        crate::focus_ring::sweep::prepare(&app, cx);
        let repo = cx.read(|cx| app.read(cx).repository.clone().unwrap());
        let panel = cx.update(|window, cx| {
            cx.new(|cx| Panel::new(app.downgrade(), repo, "main".into(), window, cx))
        });
        app.read_with(cx, |app, _| app.operations.submit(|| Ok(())))
            .await
            .unwrap()
            .unwrap();
        cx.executor().run_until_parked();
        let pull = github::review::fixture_pull(false);
        let captured = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let threads = Client(UiTransport::Fixture { moved: false })
            .review_threads(&captured, None, &OperationControl::default())
            .unwrap();
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.destination.update(cx, |input, cx| {
                    input.set_value("gitturtle-fixture/native-review", window, cx)
                });
                panel.rows = vec![pull.clone()];
                panel.review.fixture = true;
                panel.account = Some("offline-reviewer".into());
                panel.receive_threads(threads, None);
                panel.close(window, cx);
            });
            app.update(cx, |app, cx| app.open_github(window, cx));
        });
        cx.executor().run_until_parked();
        (fixture, app, panel, pull, cx)
    }

    /// Shows `pull`'s Overview at `interface` pt in a `width` × `height` window, the body and
    /// the conversation list at their tops and focus on the conversations pane, as the panel
    /// opens on it.
    fn show_overview(
        panel: &Entity<Panel>,
        pull: &PullRequest,
        (interface, width, height): (u8, f32, f32),
        cx: &mut VisualTestContext,
    ) {
        crate::focus_ring::sweep::interface_size(cx, interface);
        cx.simulate_resize(size(px(width), px(height)));
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.pull = Some(pull.clone());
                panel.review.section = Section::Overview;
                panel.review.show_list = false;
                panel.review.body_scroll.set_offset(point(px(0.), px(0.)));
                panel
                    .conversations
                    .list_scroll
                    .set_offset(point(px(0.), px(0.)));
                panel.focus_visible_section(window, cx);
                cx.notify();
            })
        });
        for _ in 0..3 {
            crate::width_walk::frame(cx);
        }
    }

    /// The lines of text the last frame drew inside `container`, a box that clips them, that
    /// its top edge cuts: each line's run, marked by the window's root, starts above the edge
    /// and ends below it by more than half a pixel.
    fn lines_cut_at_top(
        cx: &mut VisualTestContext,
        container: Bounds<Pixels>,
    ) -> Vec<Bounds<Pixels>> {
        let mark = gpui_kit::Background::from(crate::width_walk::text_mark());
        let scale = cx.update(|window, _| window.scale_factor());
        let clip = |value: gpui_kit::ScaledPixels| px(value.as_f32() / scale);
        let slack = px(0.5);
        quads(cx)
            .into_iter()
            .filter(|(line, quad)| {
                let mask = quad.content_mask.bounds;
                quad.background == mark
                    && clip(mask.top()) >= container.top() - slack
                    && clip(mask.bottom()) <= container.bottom() + slack
                    && line.left() >= container.left() - px(1.)
                    && line.right() <= container.right() + px(1.)
                    && line.top() < container.top() - slack
                    && line.bottom() > container.top() + slack
            })
            .map(|(line, _)| line)
            .collect()
    }

    /// A line the last frame drew in the panel's body ([`frame_lines`]): what it is, where it
    /// lies, its place in the body's content (its left edge and its top with the body at its
    /// top, in half pixels), which names it from one stop to the next, and whether a list
    /// inside the body holds it, whose own scroll moves it.
    #[derive(Clone, Debug)]
    struct FrameLine {
        name: String,
        top: Pixels,
        bottom: Pixels,
        place: (i32, i32),
        nested: bool,
    }

    /// The lists and clips inside the panel's body that scroll or cut their own rows.
    const BODY_LISTS: [&str; 10] = [
        "github-pr-list",
        "github-threads",
        "github-file-rows",
        "github-patch-rows",
        "github-source-text",
        "github-collected-comments",
        "github-confirm-text-scroll",
        "github-saved-drafts",
        "github-attempt-list",
        "github-reply-outcome",
    ];

    /// The selector of the part of the body named `part` (`review::revealing`), made once for
    /// the whole test run.
    fn part_selector(part: &str) -> &'static str {
        thread_local! {
            static MADE: RefCell<std::collections::HashMap<String, &'static str>> =
                RefCell::default();
        }
        MADE.with(|made| {
            *made
                .borrow_mut()
                .entry(part.to_string())
                .or_insert_with(|| format!("github-part-{part}").leak())
        })
    }

    /// `count` selectors `{prefix}{index}`, each made once for the whole test run.
    fn indexed(prefix: &'static str, count: usize) -> Vec<&'static str> {
        thread_local! {
            static MADE: RefCell<std::collections::HashMap<(&'static str, usize), &'static str>> =
                RefCell::default();
        }
        (0..count)
            .map(|index| {
                MADE.with(|made| {
                    *made
                        .borrow_mut()
                        .entry((prefix, index))
                        .or_insert_with(|| format!("{prefix}{index}").leak())
                })
            })
            .collect()
    }

    /// The body and the lines the last frame drew in it, after a fresh draw, read from the
    /// frame rather than from the body's record of its lines ([`review::Blocks::lines`]), so a
    /// surface that reports no lines shows as lines a reveal of the body cuts: each line of
    /// text the dialog draws, its run marked by the window's root (`width_walk::text_mark`,
    /// [`fixture_panel`]); each control of one line, a labelled Button, a field or a row of the
    /// PR, changed-file and patch lists, its frame as it drew it (`review::drawn_one_lines`);
    /// and each row of a multi-line editor drawn, Source's among them, read from the editor's
    /// layout. What a list or a clip inside the body hides is that list's or clip's, as the
    /// body counts it, and of what one of them cuts, the part it shows. Only lines drawn count:
    /// what lies out of the body's view is not drawn at all ([`verified_stop`]).
    ///
    /// Read from accessibility nodes and debug selectors alone, as they first were, the lines
    /// left out every Button, which writes no node a test reads, and took its label's run,
    /// 12 px at 13 pt, for its line; left out the part a list shows of a row it cuts, which the
    /// body counts; and counted the History page's text beneath the dialog where its clips lay
    /// inside the body.
    fn frame_lines(
        cx: &mut VisualTestContext,
        panel: &Entity<Panel>,
    ) -> (Bounds<Pixels>, Vec<FrameLine>) {
        use crate::native_accessibility::written;
        written::forget();
        crate::tags::tests::draw(cx);
        let body = cx.debug_bounds("github-panel").expect("the body");
        let scroll = cx.read(|cx| panel.read(cx).review.body_scroll.clone());
        let origin = body.top() + scroll.offset().y;
        let scale = cx.update(|window, _| window.scale_factor());
        let slack = px(0.5);
        let lists: Vec<Bounds<Pixels>> = BODY_LISTS
            .iter()
            .filter_map(|list| cx.debug_bounds(list))
            .collect();
        let mut lines = Vec::new();
        // The part of `line` that `clip`, the clip it is drawn in, shows, but where that clip's
        // edge is the body's own, which a reveal moves; none where the clip lies outside the
        // body or shows less than layout's rounding.
        let visible = |line: Bounds<Pixels>, clip: Bounds<Pixels>| {
            if clip.top() < body.top() - slack
                || clip.bottom() > body.bottom() + slack
                || clip.left() < body.left() - slack
                || clip.right() > body.right() + slack
            {
                return None;
            }
            let top = if clip.top() > body.top() + slack {
                line.top().max(clip.top())
            } else {
                line.top()
            };
            let bottom = if clip.bottom() < body.bottom() - slack {
                line.bottom().min(clip.bottom())
            } else {
                line.bottom()
            };
            (bottom - top > slack)
                .then(|| Bounds::from_corners(point(line.left(), top), point(line.right(), bottom)))
        };
        let mut push = |name: String, bounds: Bounds<Pixels>| {
            let nested = lists.iter().any(|list| list.intersects(&bounds));
            let half = |value: Pixels| (f32::from(value) * 2.).round() as i32;
            lines.push(FrameLine {
                name,
                top: bounds.top(),
                bottom: bounds.bottom(),
                place: (half(bounds.left()), half(bounds.top() - origin)),
                nested,
            });
        };
        // The texts, read from the accessibility nodes the frame wrote, which name the lines
        // in them.
        let nodes: Vec<(String, Bounds<Pixels>)> = written::inside("github-panel")
            .into_iter()
            .filter_map(|id| {
                let text = written::get(&id)?;
                let layout = text.layout?;
                matches!(text.node.role(), Role::Label | Role::Button).then(|| {
                    let bounds = Bounds::from_corners(
                        point(px(layout.x0 as f32 / scale), px(layout.y0 as f32 / scale)),
                        point(px(layout.x1 as f32 / scale), px(layout.y1 as f32 / scale)),
                    );
                    (id, bounds)
                })
            })
            .collect();
        let named = |line: Bounds<Pixels>, otherwise: &str| {
            nodes
                .iter()
                .filter(|(_, bounds)| bounds.contains(&line.center()))
                .min_by(|a, b| {
                    let area = |bounds: &Bounds<Pixels>| {
                        f32::from(bounds.size.height) * f32::from(bounds.size.width)
                    };
                    area(&a.1).total_cmp(&area(&b.1))
                })
                .map_or(otherwise.to_string(), |(id, _)| id.clone())
        };
        // Each line of text the dialog draws above its backdrop, the part of it its clips show.
        let mark = gpui_kit::Background::from(crate::width_walk::text_mark());
        let logical = |value: gpui_kit::ScaledPixels| px(value.as_f32() / scale);
        let beneath = backdrop(cx);
        for (line, quad) in quads(cx) {
            if quad.background != mark || beneath.is_some_and(|backdrop| quad.order < backdrop) {
                continue;
            }
            let mask = quad.content_mask.bounds;
            let clip = Bounds::from_corners(
                point(logical(mask.left()), logical(mask.top())),
                point(logical(mask.right()), logical(mask.bottom())),
            );
            if let Some(shown) = visible(line, clip) {
                let name = format!("{} text at {:?}", named(line, "text"), line.left());
                push(name, shown);
            }
        }
        // Each control of one line, its frame, the part of it its clips show, where they show
        // some, as GPUI draws a quad only where its clip shows some of it: a list's row it has
        // scrolled out of itself, laid out all the same, is none.
        for (frame, clip) in review::drawn_one_lines() {
            if frame.intersects(&clip)
                && let Some(shown) = visible(frame, clip)
            {
                let name = format!("{} control at {:?}", named(frame, "a"), frame.left());
                push(name, shown);
            }
        }
        // Each row of a multi-line editor drawn, as the editor draws it into its padding, the
        // part of it inside its box and the lists and clips it lies in.
        type Layout = (Bounds<Pixels>, Option<Bounds<Pixels>>, Option<Pixels>);
        let editors: Vec<(&str, Layout)> = cx.read(|cx| {
            let panel = panel.read(cx);
            let textarea = |editor: &Entity<TextareaState>| {
                let editor = editor.read(cx);
                (
                    editor.input_bounds(),
                    editor.text_bounds(),
                    editor.line_height(),
                )
            };
            let mut editors = vec![
                ("github-review-summary", textarea(&panel.body)),
                ("github-inline-composer", textarea(&panel.review.input)),
                (
                    "github-reply-composer",
                    textarea(&panel.conversations.input),
                ),
            ];
            let code = [
                ("github-source-text", panel.review.source.as_ref()),
                ("github-part-description", panel.description.as_ref()),
            ];
            for (drawn, editor) in code {
                if let Some(editor) = editor {
                    let editor = editor.read(cx);
                    editors.push((
                        drawn,
                        (
                            editor.input_bounds(),
                            editor.text_bounds(),
                            editor.line_height(),
                        ),
                    ));
                }
            }
            editors
        });
        for (drawn, (view, text, line)) in editors {
            let (Some(frame), Some(text), Some(line)) = (cx.debug_bounds(drawn), text, line) else {
                continue;
            };
            let mut top = text.top();
            let mut row = 0;
            // The rows it draws, to two past the row at its viewport's foot, that start inside
            // its box, the part of each inside the box.
            while top < frame.bottom().min(view.bottom() + line * 2.) {
                if top + line > view.top() {
                    let (shown, ends) = lists
                        .iter()
                        .filter(|list| list.left() < view.right() && list.right() > view.left())
                        .filter(|list| list.top() < top + line && list.bottom() > top)
                        .fold(
                            (top.max(frame.top()), (top + line).min(frame.bottom())),
                            |(shown, ends), list| (shown.max(list.top()), ends.min(list.bottom())),
                        );
                    if ends - shown > slack {
                        let part = Bounds::from_corners(
                            point(view.left(), shown),
                            point(view.right(), ends),
                        );
                        push(format!("{drawn} row {row}"), part);
                    }
                }
                top += line;
                row += 1;
            }
        }
        (body, lines)
    }

    /// The names of `lines` cut at the edges of `body`: a line an edge falls inside, by more
    /// than half a pixel either way, layout's rounding.
    fn cut_at_edges(body: Bounds<Pixels>, lines: &[FrameLine]) -> Vec<String> {
        cut_at_edges_by(body, lines, px(0.5))
    }

    /// The names of `lines` cut at the edges of `body` by more than `within` either way.
    fn cut_at_edges_by(body: Bounds<Pixels>, lines: &[FrameLine], within: Pixels) -> Vec<String> {
        lines
            .iter()
            .filter(|line| {
                [body.top(), body.bottom()]
                    .into_iter()
                    .any(|edge| line.top < edge - within && line.bottom > edge + within)
            })
            .map(|line| format!("{} {:?}..{:?}", line.name, line.top, line.bottom))
            .collect()
    }

    /// Where the top edge of `body`, scrolled from `first` (its content's top) to `last` (as
    /// far as it scrolls), could stop, in this frame's coordinates, keeping `top..bottom` whole
    /// with neither edge inside one of `lines` by more than the reveal's hundredth of a pixel:
    /// every place the frame offers, each line's top and foot (the top edge between lines),
    /// each less the body's height (the bottom edge between lines), each line's top less
    /// `ring` (a block a line starts, its ring's room above it) and both ends of the content
    /// ([`assert_reveal_rule`]). The ends of the content allow layout's rounding, and
    /// `top..bottom`, the focus with its ring, stays whole to the reveal's hundredth: a place
    /// half a pixel inside it cuts the ring ([`review::clear_of_lines`]).
    fn clear_stops(
        body: Bounds<Pixels>,
        lines: &[FrameLine],
        (top, bottom): (Pixels, Pixels),
        ring: Pixels,
        (first, last): (Pixels, Pixels),
    ) -> Vec<Pixels> {
        let slack = px(0.5);
        let within = px(0.01);
        let height = body.size.height;
        let inside = |edge: Pixels| {
            lines
                .iter()
                .any(|line| line.top < edge - within && line.bottom > edge + within)
        };
        lines
            .iter()
            .flat_map(|line| {
                [
                    line.top,
                    line.bottom,
                    line.top - height,
                    line.bottom - height,
                    line.top - ring,
                ]
            })
            .chain([first, last])
            .filter(|edge| {
                *edge >= first - slack
                    && *edge <= last + slack
                    && *edge <= top + within
                    && *edge + height >= bottom - within
                    && !inside(*edge)
                    && !inside(*edge + height)
            })
            .collect()
    }

    /// What the reveal that brought the body to this frame left cut where it need not have
    /// ([`sweep_reveals`]), at the stop named `stop` whose focus drew `ring`: the lines drawn
    /// cut at the edges of `body` (`drawn`, [`frame_lines`]), where a place the body's top
    /// edge can take keeps the focus whole with its ring, and what the reveal kept whole, the
    /// part with its ring's room (`review::Blocks::kept`), and draws no line cut there
    /// ([`clear_stops`], [`verified_stop`]). `Err(())` for a part too tall for the body with its
    /// ring's room, which keeps the half of the body on the side it shows, as the reveal's own
    /// tests hold; `Ok(None)` where nothing is cut or no such place exists. The part was once
    /// found by its debug selector, which the PR list's and the destination field's own
    /// selectors replace: the sweep held those stops to the focused control's ring alone, and
    /// took a reveal that kept the list's room below its last row for a needless cut.
    fn needless_cut(
        cx: &mut VisualTestContext,
        panel: &Entity<Panel>,
        (body, drawn): (Bounds<Pixels>, &[FrameLine]),
        ring: &[crate::focus_ring::PaintedOutline],
        stop: &str,
    ) -> Result<Option<String>, ()> {
        let cut = cut_at_edges(body, drawn);
        let Some(mut shown) = ring
            .iter()
            .map(|outline| outline.bounds)
            .reduce(|a, b| a.union(&b))
        else {
            return Ok(None);
        };
        if cut.is_empty() {
            return Ok(None);
        }
        let room = cx.update(|_, cx| appearance::button_ring_room(cx));
        let scroll = cx.read(|cx| panel.read(cx).review.body_scroll.clone());
        let kept =
            cx.update(|window, cx| panel.read(cx).review.body_blocks.kept(&scroll, window, cx));
        if let Some((top, bottom)) = kept {
            if bottom - top > body.size.height + px(0.5) {
                return Err(());
            }
            shown = shown.union(&Bounds::from_corners(
                point(shown.left(), top),
                point(shown.right(), bottom),
            ));
        }
        let first = body.top() + scroll.offset().y;
        let last = first + scroll.max_offset().y;
        let stops = clear_stops(
            body,
            drawn,
            (shown.top(), shown.bottom()),
            room,
            (first, last),
        );
        Ok(verified_stop(cx, panel, body, stops).map(|clear| {
            format!(
                "{stop}: {cut:?} cut at the edges of the body {body:?}, the focus {shown:?} \
                 whole, where its top edge could stop clear of every line at {clear:?}"
            )
        }))
    }

    /// The scene order of the open dialog's backdrop in the last frame, the fill in the kit's
    /// overlay colour over the whole window, as the focus sweep reads it (`focus_ring::sweep`):
    /// what the page beneath the dialog paints lies below it. None with no backdrop drawn.
    fn backdrop(cx: &mut VisualTestContext) -> Option<u32> {
        let overlay = cx.read(|cx| {
            gpui_kit::Background::from(gpui_kit::component::Theme::global(cx).colors.overlay)
        });
        cx.update(|window, _| {
            let scale = window.scale_factor();
            let viewport = window.viewport_size();
            window
                .painted_quads()
                .into_iter()
                .filter(|quad| {
                    !overlay.is_transparent()
                        && quad.background == overlay
                        && quad.bounds.size.width.as_f32() >= viewport.width.as_f32() * scale - 1.
                        && quad.bounds.size.height.as_f32() >= viewport.height.as_f32() * scale - 1.
                })
                .map(|quad| quad.order)
                .max()
        })
    }

    /// The first of `stops`, places for the top edge of `body` in this frame's coordinates
    /// ([`clear_stops`]), nearest first, where the body scrolled there draws no line cut at
    /// either of its edges by more than the reveal's hundredth of a pixel ([`frame_lines`],
    /// [`review::clear_of_lines`]), the body drawn again where it stood afterwards: a place on
    /// the half-pixel grid the frame lays lines out on moves each line by as much, so where it
    /// meets a line's edge it meets it exactly. Allowed half a pixel, as a reveal's landing is,
    /// a place cut the second line of Drafts stay on this computer by half a pixel at 13 pt in
    /// Overview. The lines a place is chosen by are those this frame drew, and a frame
    /// draws nothing out of the body's view: a place clear of them can cut a line the body
    /// scrolled there shows, as the sweep's places once did, at 18 pt in a 461 × 490 window,
    /// where the top edge stopping 26 px above the review summary in Review would have cut
    /// the review's choices above it.
    fn verified_stop(
        cx: &mut VisualTestContext,
        panel: &Entity<Panel>,
        body: Bounds<Pixels>,
        mut stops: Vec<Pixels>,
    ) -> Option<Pixels> {
        let scroll = cx.read(|cx| panel.read(cx).review.body_scroll.clone());
        let stood = scroll.offset();
        stops.sort_by(|a, b| {
            (*a - body.top())
                .abs()
                .as_f32()
                .total_cmp(&(*b - body.top()).abs().as_f32())
        });
        stops.dedup_by(|a, b| (*a - *b).abs() < px(0.01));
        let slack = px(0.5);
        let found = stops.into_iter().find(|at| {
            let offset = stood.y - (*at - body.top());
            scroll.set_offset(point(stood.x, offset));
            let (there, lines) = frame_lines(cx, panel);
            (scroll.offset().y - offset).abs() <= slack
                && cut_at_edges_by(there, &lines, px(0.01)).is_empty()
        });
        scroll.set_offset(stood);
        crate::tags::tests::draw(cx);
        found
    }

    /// The conversation list and the lines drawn in it, each named, with where it lies, after
    /// a fresh draw: each line of every text in it and each of its labelled Buttons, read from
    /// the accessibility nodes the frame wrote, the rows it has scrolled out of itself among
    /// them, since its own scroll can bring them into view. A text whose box is more than a
    /// whole number of lines, as a conversation's status, padded inside its surface the same
    /// above and below, counts its lines where its text draws them, that padding inside its
    /// box: a status 369.5..395 draws its text 372.5..392 at 13 pt, where the list counts its
    /// line too, and counted as its box it took the list's foot at the text's top for a cut.
    fn list_lines(cx: &mut VisualTestContext) -> (Bounds<Pixels>, Vec<(String, Pixels, Pixels)>) {
        use crate::native_accessibility::written;
        written::forget();
        crate::tags::tests::draw(cx);
        let list = cx.debug_bounds("github-threads").expect("the list");
        let line = label_line(cx);
        let scale = cx.update(|window, _| window.scale_factor());
        let mut lines = Vec::new();
        for id in written::inside("github-threads") {
            let Some(text) = written::get(&id) else {
                continue;
            };
            let Some(layout) = text.layout else {
                continue;
            };
            let (top, bottom) = (px(layout.y0 as f32 / scale), px(layout.y1 as f32 / scale));
            match text.node.role() {
                Role::Button => lines.push((id, top, bottom)),
                Role::Label => {
                    let count = ((bottom - top) / line + 0.05).floor();
                    if count < 1. {
                        lines.push((id, top, bottom));
                        continue;
                    }
                    // What the lines leave of the box, its padding, half above them.
                    let padding = ((bottom - top) - line * count) / 2.;
                    for index in 0..count as usize {
                        let at = top + padding + line * index as f32;
                        lines.push((format!("{id} line {}", index + 1), at, at + line));
                    }
                }
                _ => {}
            }
        }
        (list, lines)
    }

    /// Tab down through a PR's conversations on the offline fixture, at 13, 17 and 18 pt in
    /// windows 430 to 500 px wide, every 10 px, and 490 px tall, where the conversation list is
    /// shorter than its conversations: wherever Tab scrolls the list onto a conversation's actions,
    /// neither of the list's edges falls inside a line drawn in it (each line of its text and
    /// each labelled Button; half a pixel either way is layout's rounding), unless no place
    /// the list's top edge can take keeps the actions whole with their ring's room and both
    /// edges clear: each line's top and foot, each less the list's height, each line's top less
    /// the ring's room and both ends of the list's content. A conversation's status counts its
    /// line where its text draws, inside its padding ([`list_lines`]). The list's reveals once
    /// collected no lines of their own (FU15-D's review): they stopped wherever the block
    /// starts put them, and at 13 pt in a 480 × 490 window Tab onto the third conversation's
    /// actions left its permissions line, 274.5..294, cut at the list's foot, 279.5.
    #[gpui::test]
    async fn conversation_reveals_leave_no_line_cut_at_the_lists_edges(cx: &mut TestAppContext) {
        let (_fixture, _app, panel, pull, cx) = fixture_panel(cx).await;
        let list_scroll = cx.read(|cx| panel.read(cx).conversations.list_scroll.clone());
        // The selectors of the first conversations' actions, made once for the whole test.
        let names: Vec<&'static str> = (0..16)
            .map(|index| &*format!("github-thread-actions-{index}").leak())
            .collect();
        let mut moves = 0;
        let sizes = [13, 17, 18].into_iter().flat_map(|size| {
            (430..=500)
                .step_by(10)
                .map(move |width| (size, width as f32, 490.))
        });
        for sizes in sizes {
            let at = format!("{} pt in {} × {}", sizes.0, sizes.1, sizes.2);
            show_overview(&panel, &pull, sizes, cx);
            let ring = cx.update(|_, cx| appearance::button_ring_room(cx));
            let mut offset = list_scroll.offset().y;
            for stop in 0..60 {
                cx.simulate_keystrokes("tab");
                for _ in 0..3 {
                    crate::width_walk::frame(cx);
                }
                let part = cx.update(|window, _| {
                    window
                        .context_stack()
                        .iter()
                        .rev()
                        .find_map(|context| context.get(review::PART).cloned())
                });
                if part.as_ref().is_some_and(|part| part.as_ref() == "banner") {
                    break;
                }
                let now = list_scroll.offset().y;
                let moved = now != offset;
                offset = now;
                // At the list's top, a reveal keeps the box's top (`review::clear_of_lines`).
                if !moved || now == px(0.) {
                    continue;
                }
                let actions = names.iter().enumerate().find_map(|(index, name)| {
                    let actions = cx.debug_bounds(name)?;
                    let focused = cx.update(|window, _| {
                        window.context_stack().iter().any(|context| {
                            context.get("GitTurtleGithubConversation")
                                == Some(&index.to_string().into())
                        })
                    });
                    focused.then_some(actions)
                });
                let Some(actions) = actions else {
                    continue;
                };
                moves += 1;
                let here = format!("{at}, stop {stop}");
                let (list, lines) = list_lines(cx);
                let (top, bottom) = (actions.top() - ring, actions.bottom() + ring);
                let slack = px(0.5);
                assert!(
                    top >= list.top() - slack && bottom <= list.bottom() + slack,
                    "{here}: Tab shows the actions {actions:?} whole in the list {list:?}"
                );
                let inside = |edge: Pixels, within: Pixels| {
                    lines.iter().filter(move |(_, top, bottom)| {
                        *top < edge - within && *bottom > edge + within
                    })
                };
                let cut: Vec<_> = inside(list.top(), slack)
                    .chain(inside(list.bottom(), slack))
                    .map(|(name, top, bottom)| format!("{name} {top:?}..{bottom:?}"))
                    .collect();
                if cut.is_empty() {
                    continue;
                }
                let height = list.size.height;
                let first = list.top() + now;
                let last = first + list_scroll.max_offset().y;
                let within = px(0.01);
                let stops: Vec<Pixels> = lines
                    .iter()
                    .flat_map(|(_, top, bottom)| {
                        [*top, *bottom, *top - height, *bottom - height, *top - ring]
                    })
                    .chain([first, last])
                    .filter(|edge| {
                        *edge >= first - slack
                            && *edge <= last + slack
                            && *edge <= top + slack
                            && *edge + height >= bottom - slack
                            && inside(*edge, within).next().is_none()
                            && inside(*edge + height, within).next().is_none()
                    })
                    .collect();
                assert_eq!(
                    stops,
                    Vec::<Pixels>::new(),
                    "{here}: {cut:?} cut at the edges of the list {list:?} where its top edge \
                     could stop clear of every line"
                );
            }
        }
        assert!(moves > 0, "Tab scrolls the conversation list onto actions");
    }

    /// The caret the last frame drew inside `view`, an editor's viewport, after a fresh draw:
    /// the one fill there at most 3 px wide and about as tall as a line of text. It marks the
    /// row it stands on where the frame drew it, apart from the editor's record of its layout,
    /// which the body reads ([`review::Blocks::lines`]).
    fn drawn_caret(cx: &mut VisualTestContext, view: Bounds<Pixels>) -> Option<Bounds<Pixels>> {
        crate::tags::tests::draw(cx);
        let slack = px(0.5);
        let carets: Vec<Bounds<Pixels>> = quads(cx)
            .into_iter()
            .map(|(bounds, _)| bounds)
            .filter(|bounds| {
                bounds.size.width > px(0.)
                    && bounds.size.width <= px(3.)
                    && bounds.size.height >= px(8.)
                    && bounds.size.height <= px(48.)
                    && bounds.left() >= view.left() - slack
                    && bounds.right() <= view.right() + slack
                    && bounds.top() >= view.top() - slack
                    && bounds.bottom() <= view.bottom() + slack
            })
            .collect();
        assert!(carets.len() <= 1, "one caret in {view:?}: {carets:?}");
        carets.first().copied()
    }

    /// A multi-line editor reports each of its rows as a line of the body, at 13 and 18 pt in
    /// a 461 × 490 window, so no reveal of the body stops with a row of its text cut at either
    /// edge: a PR's review summary, holding twelve lines of draft in its 130 px, and Overview's
    /// opened-PR description, a 120 px read-only editor holding twelve lines of Markdown, each
    /// walked row by row with the keyboard, the summary from its last row up and the
    /// description from its first down, so each scrolls inside itself. Each row the frame
    /// draws the caret on, whole in the editor and in the body, lies in one line of the body
    /// no taller than two rows. The editors' rows once reported no lines, and the body's
    /// reveals took an editor for room between lines, where either edge could stop (FU15-D's
    /// review); the description reported none until FU18-F's review.
    #[gpui::test]
    async fn a_multi_line_editors_rows_are_lines_of_the_body(cx: &mut TestAppContext) {
        let (_fixture, _app, panel, pull, cx) = fixture_panel(cx).await;
        let summary = cx.read(|cx| panel.read(cx).body.clone());
        let text: String = (1..=12)
            .map(|line| format!("Line {line} of the text"))
            .collect::<Vec<_>>()
            .join("\n");
        let slack = px(0.5);
        for sizes in [(13, 461., 490.), (18, 461., 490.)] {
            for description in [false, true] {
                let at = format!(
                    "{} at {} pt in {} × {}",
                    if description {
                        "the description"
                    } else {
                        "the summary"
                    },
                    sizes.0,
                    sizes.1,
                    sizes.2
                );
                show_overview(&panel, &pull, sizes, cx);
                let text = text.clone();
                let summary = summary.clone();
                // The editor's viewport, as it last painted.
                type Viewport = Box<dyn Fn(&App) -> Bounds<Pixels>>;
                let layout: Viewport = cx.update(|window, cx| {
                    // An inactive window draws no caret.
                    window.activate_window();
                    panel.update(cx, |panel, cx| {
                        if description {
                            let editor = text::editor(&text, "markdown", None, window, cx);
                            panel.description = Some(editor.clone());
                            window.focus(&editor.read(cx).focus_handle(cx), cx);
                            Box::new(move |cx: &App| editor.read(cx).input_bounds()) as Viewport
                        } else {
                            summary.update(cx, |input, cx| input.set_value(text, window, cx));
                            panel.review.section = Section::Review;
                            window.focus(&summary.read(cx).focus_handle(cx), cx);
                            Box::new(move |cx: &App| summary.read(cx).input_bounds())
                        }
                    })
                });
                for _ in 0..3 {
                    crate::width_walk::frame(cx);
                }
                // The editor's top in view, where the body draws its rows above its foot.
                let part = if description {
                    "github-part-description"
                } else {
                    "github-review-summary"
                };
                let body = cx.debug_bounds("github-panel").expect("the body");
                let field = cx.debug_bounds(part).expect("the editor");
                let scroll = cx.read(|cx| panel.read(cx).review.body_scroll.clone());
                let offset = (scroll.offset().y - (field.top() - body.top() - px(20.)))
                    .clamp(-scroll.max_offset().y, px(0.));
                scroll.set_offset(point(px(0.), offset));
                let key = if description { "down" } else { "up" };
                let (mut checked, mut tops) = (0, Vec::new());
                for row in 0..12 {
                    if row > 0 {
                        cx.simulate_keystrokes(key);
                    }
                    for _ in 0..2 {
                        crate::width_walk::frame(cx);
                    }
                    let view = cx.read(|cx| layout(cx));
                    let Some(caret) = drawn_caret(cx, view) else {
                        continue;
                    };
                    tops.push(caret.top());
                    let body = cx.debug_bounds("github-panel").expect("the body");
                    if caret.top() < body.top() - slack || caret.bottom() > body.bottom() + slack {
                        continue;
                    }
                    let lines = panel.read_with(cx, |panel, cx| panel.review.body_blocks.lines(cx));
                    let held = lines.iter().any(|(top, bottom)| {
                        *top <= caret.top() + slack
                            && *bottom >= caret.bottom() - slack
                            && *bottom - *top < caret.size.height * 2.
                    });
                    assert!(
                        held,
                        "{at}, row {row}: the row the caret {caret:?} stands on is a line of the \
                         body: {lines:?}"
                    );
                    checked += 1;
                }
                assert!(
                    checked >= 6,
                    "{at}: the caret stood on {checked} rows in view"
                );
                // Past the rows in view, the caret stays at the editor's edge while its rows
                // scroll under it.
                assert!(
                    tops.windows(2)
                        .any(|pair| (pair[0] - pair[1]).abs() < slack),
                    "{at}: the editor scrolls inside itself as the caret moves: {tops:?}"
                );
            }
        }
    }

    /// A row a multi-line editor draws below its viewport, starting in its bottom padding, is a
    /// line of the body inside the editor's box, at 13 and 18 pt in a 461 × 490 window: the
    /// review summary, scrolled so its ninth row starts 2 px below its viewport, its caret on
    /// that row. The body counted rows only to the viewport's foot, so a reveal could stop with
    /// the body's edge through that row's text in the padding (FU20-GH2's review: the summary
    /// scrolled 14 px at 13 pt drew the row from 228.5 px, its viewport ending at 226.5 and its
    /// box at 235.5).
    #[gpui::test]
    async fn an_editors_row_in_its_bottom_padding_is_a_line_of_the_body(cx: &mut TestAppContext) {
        let (_fixture, _app, panel, pull, cx) = fixture_panel(cx).await;
        let summary = cx.read(|cx| panel.read(cx).body.clone());
        let rows: Vec<String> = (1..=12)
            .map(|line| format!("Line {line} of the text"))
            .collect();
        let scroll = cx.read(|cx| panel.read(cx).review.body_scroll.clone());
        let slack = px(0.5);
        for interface in [13, 18] {
            let at = format!("the summary at {interface} pt in 461 × 490");
            show_overview(&panel, &pull, (interface, 461., 490.), cx);
            cx.update(|window, cx| {
                // An inactive window draws no caret.
                window.activate_window();
                panel.update(cx, |panel, _| panel.review.section = Section::Review);
                let text = rows.join("\n");
                summary.update(cx, |input, cx| {
                    input.set_value(text, window, cx);
                    input.set_selected_range(0..0, cx);
                });
                window.focus(&summary.read(cx).focus_handle(cx), cx);
            });
            for _ in 0..3 {
                crate::width_walk::frame(cx);
            }
            // The summary's top 20 px below the body's top.
            let body = cx.debug_bounds("github-panel").expect("the body");
            let field = cx
                .debug_bounds("github-review-summary")
                .expect("the editor");
            let offset = (scroll.offset().y - (field.top() - body.top() - px(20.)))
                .clamp(-scroll.max_offset().y, px(0.));
            scroll.set_offset(point(px(0.), offset));
            for _ in 0..3 {
                crate::width_walk::frame(cx);
            }
            // The first row below the viewport's foot starts 2 px below it, the caret on it.
            let (view, line) = cx.read(|cx| {
                let summary = summary.read(cx);
                (
                    summary.input_bounds(),
                    summary.line_height().expect("painted"),
                )
            });
            let row = ((view.size.height + px(2.)) / line).ceil() as usize;
            let by = line * row as f32 - view.size.height - px(2.);
            let start: usize = rows[..row].iter().map(|text| text.len() + 1).sum();
            cx.update(|_, cx| {
                summary.update(cx, |editor, cx| {
                    editor.set_selected_range(start..start, cx);
                    editor.set_scroll_offset(point(px(0.), -by), cx);
                })
            });
            for _ in 0..3 {
                crate::width_walk::frame(cx);
            }
            let view = cx.read(|cx| summary.read(cx).input_bounds());
            let field = cx
                .debug_bounds("github-review-summary")
                .expect("the editor");
            crate::tags::tests::draw(cx);
            let carets: Vec<Bounds<Pixels>> = quads(cx)
                .into_iter()
                .map(|(bounds, _)| bounds)
                .filter(|bounds| {
                    bounds.size.width > px(0.)
                        && bounds.size.width <= px(3.)
                        && bounds.size.height >= px(8.)
                        && bounds.size.height <= px(48.)
                        && bounds.left() >= view.left() - slack
                        && bounds.right() <= view.right() + slack
                        && bounds.top() >= view.bottom() - slack
                        && bounds.top() < field.bottom()
                })
                .collect();
            let [caret] = carets[..] else {
                panic!("{at}: one caret below the viewport {view:?} in {field:?}: {carets:?}");
            };
            let body = cx.debug_bounds("github-panel").expect("the body");
            assert!(
                field.bottom() < body.bottom(),
                "{at}: the summary {field:?} ends inside the body {body:?}"
            );
            let lines = panel.read_with(cx, |panel, cx| panel.review.body_blocks.lines(cx));
            assert!(
                lines.iter().any(|(top, bottom)| {
                    *top <= caret.top() + slack
                        && *bottom >= caret.bottom().min(field.bottom()) - slack
                        && *bottom <= field.bottom() + slack
                }),
                "{at}: the row the caret {caret:?} stands on below the viewport {view:?} is a \
                 line of the body inside the summary {field:?}: {lines:?}"
            );
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// A row a multi-line editor has scrolled partly out of its viewport is a whole line of the
    /// body, at 13 and 18 pt in a 461 × 490 window, cut only where a clip inside the body cuts
    /// it: the editor clips its rows only sideways, so the row draws whole into the editor's
    /// padding, where the body counted only the part inside the viewport, and a reveal could
    /// stop with the body's edge at the viewport's edge, through the row's text in the padding
    /// (FU20-GH's review). The review summary, scrolled 4 px inside itself, reports its first
    /// row from the top the frame draws the caret at, in the summary's top padding; Source,
    /// scrolled 4 px past its 8 px top padding, reports its first row from the top of the clip
    /// it lies in, which cuts the row's top above it.
    #[gpui::test]
    async fn an_editors_row_in_its_padding_is_a_whole_line_of_the_body(cx: &mut TestAppContext) {
        let (_fixture, app, panel, pull, cx) = fixture_panel(cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        let captured = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let files = Client(UiTransport::Fixture { moved: false })
            .files(&captured, 1, &OperationControl::default())
            .unwrap()
            .items;
        let summary = cx.read(|cx| panel.read(cx).body.clone());
        let text: String = (1..=12)
            .map(|line| format!("Line {line} of the text"))
            .collect::<Vec<_>>()
            .join("\n");
        let scroll = cx.read(|cx| panel.read(cx).review.body_scroll.clone());
        let slack = px(0.5);
        for interface in [13, 18] {
            for source in [false, true] {
                let at = format!(
                    "{} at {interface} pt in 461 × 490",
                    if source { "Source" } else { "the summary" }
                );
                show_overview(&panel, &pull, (interface, 461., 490.), cx);
                if source {
                    let (pull, files) = (pull.clone(), files.clone());
                    cx.update(|window, cx| {
                        panel.update(cx, |panel, cx| {
                            panel.pull = Some(pull);
                            panel.review.section = Section::Files;
                            panel.review.files = files;
                            panel.review.selected_file = None;
                            panel.select_review_file(0, window, cx);
                        })
                    });
                    settle_panel(&app, cx).await;
                    cx.update(|window, cx| {
                        panel.update(cx, |panel, cx| panel.review_source(1, window, cx))
                    });
                    settle_panel(&app, cx).await;
                }
                // The editor focused, its caret on its first row, and how to scroll it and read
                // its viewport as it last painted.
                type Scroll = Box<dyn Fn(Pixels, &mut App)>;
                type Viewport = Box<dyn Fn(&App) -> Bounds<Pixels>>;
                let (scroll_to, layout, part): (Scroll, Viewport, &'static str) =
                    cx.update(|window, cx| {
                        // An inactive window draws no caret.
                        window.activate_window();
                        if source {
                            let editor = panel.read(cx).review.source.clone().expect("Source");
                            editor.update(cx, |editor, cx| editor.set_selected_range(0..0, cx));
                            window.focus(&editor.read(cx).focus_handle(cx), cx);
                            let scrolled = editor.clone();
                            (
                                Box::new(move |by: Pixels, cx: &mut App| {
                                    scrolled.update(cx, |editor, cx| {
                                        editor.set_scroll_offset(point(px(0.), -by), cx)
                                    })
                                }) as Scroll,
                                Box::new(move |cx: &App| editor.read(cx).input_bounds())
                                    as Viewport,
                                "github-source-patch",
                            )
                        } else {
                            panel.update(cx, |panel, _| panel.review.section = Section::Review);
                            let text = text.clone();
                            summary.update(cx, |input, cx| {
                                input.set_value(text, window, cx);
                                input.set_selected_range(0..0, cx);
                            });
                            window.focus(&summary.read(cx).focus_handle(cx), cx);
                            let (scrolled, read) = (summary.clone(), summary.clone());
                            (
                                Box::new(move |by: Pixels, cx: &mut App| {
                                    scrolled.update(cx, |editor, cx| {
                                        editor.set_scroll_offset(point(px(0.), -by), cx)
                                    })
                                }) as Scroll,
                                Box::new(move |cx: &App| read.read(cx).input_bounds()) as Viewport,
                                "github-review-summary",
                            )
                        }
                    });
                for _ in 0..3 {
                    crate::width_walk::frame(cx);
                }
                // The editor's top 20 px below the body's top.
                let body = cx.debug_bounds("github-panel").expect("the body");
                let field = cx.debug_bounds(part).expect("the editor");
                let offset = (scroll.offset().y - (field.top() - body.top() - px(20.)))
                    .clamp(-scroll.max_offset().y, px(0.));
                scroll.set_offset(point(px(0.), offset));
                for _ in 0..3 {
                    crate::width_walk::frame(cx);
                }
                // Past its top padding, 8 px, in Source.
                let by = if source { px(12.) } else { px(4.) };
                cx.update(|_, cx| scroll_to(by, cx));
                for _ in 0..3 {
                    crate::width_walk::frame(cx);
                }
                let view = cx.read(|cx| layout(cx));
                // The caret on the first row, drawn above the viewport's top by the scroll.
                crate::tags::tests::draw(cx);
                let carets: Vec<Bounds<Pixels>> = quads(cx)
                    .into_iter()
                    .map(|(bounds, _)| bounds)
                    .filter(|bounds| {
                        bounds.size.width > px(0.)
                            && bounds.size.width <= px(3.)
                            && bounds.size.height >= px(8.)
                            && bounds.size.height <= px(48.)
                            && bounds.left() >= view.left() - slack
                            && bounds.right() <= view.right() + slack
                            && bounds.bottom() > view.top()
                            && bounds.top() < view.top()
                    })
                    .collect();
                let [caret] = carets[..] else {
                    panic!("{at}: one caret across the viewport's top {view:?}: {carets:?}");
                };
                assert!(
                    caret.top() < view.top() - slack * 2.,
                    "{at}: the editor scrolled its first row {caret:?} partly above its \
                     viewport {view:?}"
                );
                let body = cx.debug_bounds("github-panel").expect("the body");
                assert!(
                    caret.top() > body.top() && caret.bottom() < body.bottom(),
                    "{at}: the row {caret:?} lies inside the body {body:?}"
                );
                // Where the row shows: in the summary, above the viewport's top, from the row's
                // top above the caret's; in Source, from the top of the clip that cuts it.
                let clip = source.then(|| {
                    let clip = cx
                        .debug_bounds("github-source-text")
                        .expect("Source's clip");
                    assert!(
                        clip.top() > caret.top() + slack && clip.top() < view.top() - slack,
                        "{at}: Source's clip {clip:?} cuts the row {caret:?} above the viewport \
                         {view:?}"
                    );
                    clip.top()
                });
                let lines = panel.read_with(cx, |panel, cx| panel.review.body_blocks.lines(cx));
                assert!(
                    lines.iter().any(|(top, bottom)| {
                        let from = match clip {
                            Some(clip) => (*top - clip).abs() <= slack,
                            None => *top <= caret.top() + slack && *top < view.top() - slack,
                        };
                        from && *bottom >= caret.bottom() - slack
                            && *bottom - *top < caret.size.height * 2.
                    }),
                    "{at}: the row the caret {caret:?} stands on is a line of the body from \
                     {clip:?} or above the viewport {view:?}: {lines:?}"
                );
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Files reports each of its rows as a line of the body, at 13 and 18 pt in a 461 × 490
    /// window, so no reveal of the body stops with a row cut at either edge: in Changes, each
    /// row of the changed-file list and of the patch that its list shows whole, and in Source,
    /// each row of the editor its clip shows whole, the rows below the body's foot that a
    /// reveal can bring into view among them. They reported no lines, and the body's reveals
    /// took them for room between lines, where either edge could stop (FU18-E's review).
    #[gpui::test]
    async fn files_rows_are_lines_of_the_body(cx: &mut TestAppContext) {
        let (_fixture, app, panel, pull, cx) = fixture_panel(cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        let captured = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let files = Client(UiTransport::Fixture { moved: false })
            .files(&captured, 1, &OperationControl::default())
            .unwrap()
            .items;
        let file_rows: Vec<&'static str> = (0..files.len())
            .map(|index| &*format!("github-file-row-{index}").leak())
            .collect();
        let patch_rows: Vec<&'static str> = (0..64)
            .map(|index| &*format!("github-patch-line-{index}").leak())
            .collect();
        let scroll = cx.read(|cx| panel.read(cx).review.body_scroll.clone());
        let slack = px(0.5);
        for interface in [13, 18] {
            for mode in [0, 1] {
                let at = format!("mode {mode} at {interface} pt in 461 × 490");
                crate::focus_ring::sweep::interface_size(cx, interface);
                cx.simulate_resize(size(px(461.), px(490.)));
                let (pull, files) = (pull.clone(), files.clone());
                cx.update(|window, cx| {
                    panel.update(cx, |panel, cx| {
                        panel.pull = Some(pull);
                        panel.review.show_list = false;
                        panel.review.section = Section::Files;
                        panel.review.files = files;
                        panel.review.selected_file = None;
                        panel.select_review_file(0, window, cx);
                    })
                });
                settle_panel(&app, cx).await;
                if mode == 1 {
                    cx.update(|window, cx| {
                        panel.update(cx, |panel, cx| panel.review_source(1, window, cx))
                    });
                    settle_panel(&app, cx).await;
                }
                for _ in 0..3 {
                    crate::width_walk::frame(cx);
                }
                // The file column's top at the body's top, its rows in view and below.
                let body = cx.debug_bounds("github-panel").expect("the body");
                let list = cx.debug_bounds("github-file-focus").expect("the file list");
                let offset = (scroll.offset().y - (list.top() - body.top()))
                    .clamp(-scroll.max_offset().y, px(0.));
                scroll.set_offset(point(px(0.), offset));
                for _ in 0..3 {
                    crate::width_walk::frame(cx);
                }
                // Each row whole in the viewport of its list or the clip of Source's text.
                let whole_in = |row: Bounds<Pixels>, view: Option<Bounds<Pixels>>| {
                    view.is_some_and(|view| {
                        row.top() >= view.top() - slack && row.bottom() <= view.bottom() + slack
                    })
                };
                let viewport = cx.debug_bounds("github-file-rows");
                let mut rows = Vec::new();
                for selector in &file_rows {
                    if let Some(row) = cx.debug_bounds(selector)
                        && whole_in(row, viewport)
                    {
                        rows.push((selector.to_string(), row.top(), row.bottom()));
                    }
                }
                let listed = rows.len();
                // At 18 pt a file's row is 75 px tall, and the list, at most half the body,
                // shows one of them whole ([`files_fits_a_narrow_body`]); at 13 pt, several.
                let least = if interface == 18 { 1 } else { 2 };
                assert!(
                    listed >= least,
                    "{at}: the file list shows its rows: {rows:?}"
                );
                let clip = if mode == 1 {
                    let clip = cx.debug_bounds("github-source-text");
                    let (view, text, line) = cx.read(|cx| {
                        let source = panel.read(cx).review.source.clone().expect("Source");
                        let source = source.read(cx);
                        (
                            source.input_bounds(),
                            source.text_bounds(),
                            source.line_height(),
                        )
                    });
                    let (Some(text), Some(line)) = (text, line) else {
                        panic!("{at}: Source has painted");
                    };
                    let mut top = text.top();
                    while top < view.bottom() {
                        let row = Bounds::from_corners(
                            point(view.left(), top.max(view.top())),
                            point(view.right(), (top + line).min(view.bottom())),
                        );
                        if row.bottom() > row.top() && whole_in(row, clip) {
                            rows.push((format!("Source row at {top:?}"), row.top(), row.bottom()));
                        }
                        top += line;
                    }
                    clip
                } else {
                    let clip = cx.debug_bounds("github-patch-rows");
                    for selector in &patch_rows {
                        if let Some(row) = cx.debug_bounds(selector)
                            && whole_in(row, clip)
                        {
                            rows.push((selector.to_string(), row.top(), row.bottom()));
                        }
                    }
                    clip
                };
                // The file's content shows five lines at least, at 18 pt too, where the file
                // column's fixed 324 px once left it less than a row
                // ([`files_fits_a_narrow_body`]).
                assert!(
                    rows.len() >= listed + 5,
                    "{at}: the file's content shows its rows in {clip:?}: {rows:?}"
                );
                let lines = panel.read_with(cx, |panel, cx| panel.review.body_blocks.lines(cx));
                let missing: Vec<_> = rows
                    .iter()
                    .filter(|(_, top, bottom)| {
                        !lines.iter().any(|line| {
                            (line.0 - *top).abs() < px(0.01) && (line.1 - *bottom).abs() < px(0.01)
                        })
                    })
                    .map(|(name, top, bottom)| format!("{name} {top:?}..{bottom:?}"))
                    .collect();
                assert_eq!(
                    missing,
                    Vec::<String>::new(),
                    "{at}: each row Files shows is a line of the body"
                );
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Files fits a narrow body (decision, 2026-11-05), at 13, 17 and 18 pt in windows 430 to
    /// 500 px wide, every 10 px, 490 px tall, in Changes and in Source: the changed-file list
    /// stands above the file and takes at most half the body's height, its box with its ring's
    /// room inside the body; the mode row, Changes, Source, Threads and Comment on selection,
    /// wraps inside the body's width, each control's ring inside the body's right edge; and the
    /// patch or Source shows at least five whole lines, with the list's top at the body's top
    /// the two filling the body unless five lines need more. In a 1,480 × 800 window the file
    /// stays beside the list, the boxes' feet level. Beside the list in every window, the mode
    /// row ran past the body's right edge (its rings reached x 449.5 at 13 pt and 607 at 18 pt
    /// in a 461 px window, the body's edge at 423), the list's 324 px box was taller than the
    /// body, its ring's top cut, and at 18 pt the patch and Source had no height (FU18-F's
    /// sweep).
    #[gpui::test]
    async fn files_fits_a_narrow_body(cx: &mut TestAppContext) {
        let (_fixture, app, panel, pull, cx) = fixture_panel(cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        let captured = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let files = Client(UiTransport::Fixture { moved: false })
            .files(&captured, 1, &OperationControl::default())
            .unwrap()
            .items;
        let scroll = cx.read(|cx| panel.read(cx).review.body_scroll.clone());
        let room = cx.update(|_, cx| appearance::button_ring_room(cx));
        let slack = px(0.5);
        let controls = [
            "github-file-mode-0",
            "github-file-mode-1",
            "github-file-mode-2",
            "github-inline-start",
        ];
        let mut sizes: Vec<(u8, f32, f32)> = [13u8, 17, 18]
            .into_iter()
            .flat_map(|size| (430..=500).step_by(10).map(move |w| (size, w as f32, 490.)))
            .collect();
        sizes.extend([(13, 1480., 800.), (18, 1480., 800.)]);
        for (interface, width, height) in sizes {
            for mode in [0, 1] {
                let at = format!("mode {mode} at {interface} pt in {width} × {height}");
                crate::focus_ring::sweep::interface_size(cx, interface);
                cx.simulate_resize(size(px(width), px(height)));
                let (pull, files) = (pull.clone(), files.clone());
                cx.update(|window, cx| {
                    panel.update(cx, |panel, cx| {
                        panel.pull = Some(pull);
                        panel.review.show_list = false;
                        panel.review.section = Section::Files;
                        panel.review.files = files;
                        panel.review.selected_file = None;
                        panel.select_review_file(0, window, cx);
                    })
                });
                settle_panel(&app, cx).await;
                if mode == 1 {
                    cx.update(|window, cx| {
                        panel.update(cx, |panel, cx| panel.review_source(1, window, cx))
                    });
                    settle_panel(&app, cx).await;
                }
                for _ in 0..4 {
                    crate::width_walk::frame(cx);
                }
                // The list's top at the body's top, inside the ring's room.
                let body = cx.debug_bounds("github-panel").expect("the body");
                let list = cx.debug_bounds("github-file-focus").expect("the file list");
                let offset = (scroll.offset().y - (list.top() - body.top() - room))
                    .clamp(-scroll.max_offset().y, px(0.));
                scroll.set_offset(point(px(0.), offset));
                for _ in 0..3 {
                    crate::width_walk::frame(cx);
                }
                let body = cx.debug_bounds("github-panel").expect("the body");
                let list = cx.debug_bounds("github-file-focus").expect("the file list");
                let column = cx.debug_bounds("github-file-column").expect("the file");
                let (clip, line, name) = if mode == 1 {
                    let line = cx.read(|cx| {
                        let source = panel.read(cx).review.source.clone().expect("Source");
                        source.read(cx).line_height().expect("Source has painted")
                    });
                    (
                        cx.debug_bounds("github-source-text"),
                        line,
                        "github-source-patch",
                    )
                } else {
                    let row = cx.debug_bounds("github-patch-line-0").expect("a patch row");
                    (
                        cx.debug_bounds("github-patch-rows"),
                        row.size.height,
                        "github-patch",
                    )
                };
                let clip = clip.expect("the file's text");
                let content = cx.debug_bounds(name).expect("the file's box");
                // The part a reveal of the file shows, which the reveal sweep reads
                // (`part_selector`): the file's title down to its box, inside its column. The
                // column's selector once replaced the part's, so the sweep never found it.
                let part = cx
                    .debug_bounds(part_selector("file-content"))
                    .unwrap_or_else(|| panic!("{at}: the file's part"));
                let title = cx
                    .debug_bounds("github-file-title")
                    .expect("the file's title");
                assert!(
                    part.top() <= title.top() + slack
                        && part.bottom() >= content.bottom() - slack
                        && part.left() >= column.left() - slack
                        && part.right() <= column.right() + slack,
                    "{at}: the file's part {part:?} holds its title {title:?} and its box \
                     {content:?} inside its column {column:?}"
                );
                assert!(
                    clip.size.height >= line * 5. - slack,
                    "{at}: the file's text {clip:?} shows five lines of {line:?}"
                );
                for control in controls {
                    let bounds = cx.debug_bounds(control).expect("a mode control");
                    assert!(
                        bounds.left() - room >= body.left() - slack
                            && bounds.right() + room <= body.right() + slack,
                        "{at}: {control} {bounds:?} with its ring's room lies inside the body \
                         {body:?}"
                    );
                }
                if width > 1000. {
                    assert!(
                        column.left() > list.right()
                            && (content.bottom() - list.bottom()).abs() <= slack,
                        "{at}: the file {column:?}, its box {content:?}, stands beside the list \
                         {list:?}, their feet level"
                    );
                    continue;
                }
                assert!(
                    column.top() > list.bottom(),
                    "{at}: the file {column:?} stands below the list {list:?}"
                );
                assert!(
                    list.size.height <= body.size.height / 2. + slack
                        && list.top() >= body.top() + room - slack,
                    "{at}: the list {list:?} takes at most half the body {body:?}, its ring's \
                     room inside it"
                );
                // The file's box takes the rest of the body, but five lines may need more.
                let five = clip.size.height < line * 6.;
                assert!(
                    (content.bottom() - (body.bottom() - room)).abs() <= px(1.)
                        || (five && content.bottom() > body.bottom() - room),
                    "{at}: the file's box {content:?} ends at the body's foot {body:?}, less \
                     the ring's room, or shows its five lines past it"
                );
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Files fits the body a conversation's pinned top line leaves it, at 13 and 18 pt in a
    /// 461 × 490 window: with a reply open, which the section change keeps, and a notice on
    /// the top line, which then stands above the body and takes its height, the stacked list
    /// takes at most half the body drawn and the file's box ends at its foot, less the ring's
    /// room, unless five lines need more. So it does with no file selected, as Files first
    /// appears after a file page lands, the list taking half the body in the frames the change
    /// draws, and a click on a file leaves the list as tall as it was; as the notice clears with
    /// no file selected, the line gone, the list takes half the whole body in the frames that
    /// change draws, where only Files' check of the height the line drew redraws it; and with
    /// no file selected as the conversation closes, the list takes half the whole body again,
    /// up to its 324 px. Files measured the line only while a file was selected: with none, at
    /// 13 pt its list took 146.5 px of a 266.5 px body, a click on a file moved the list's foot
    /// by half the line, and a closed conversation's line kept the list 33 px short.
    #[gpui::test]
    async fn files_fits_the_body_a_pinned_top_line_leaves(cx: &mut TestAppContext) {
        let (_fixture, app, panel, pull, cx) = fixture_panel(cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        let captured = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let files = Client(UiTransport::Fixture { moved: false })
            .files(&captured, 1, &OperationControl::default())
            .unwrap()
            .items;
        let scroll = cx.read(|cx| panel.read(cx).review.body_scroll.clone());
        let room = cx.update(|_, cx| appearance::button_ring_room(cx));
        let slack = px(0.5);
        // The list's top at the body's top, inside the ring's room.
        let list_at_top = |cx: &mut VisualTestContext| {
            let body = cx.debug_bounds("github-panel").expect("the body");
            let list = cx.debug_bounds("github-file-focus").expect("the file list");
            let offset = (scroll.offset().y - (list.top() - body.top() - room))
                .clamp(-scroll.max_offset().y, px(0.));
            scroll.set_offset(point(px(0.), offset));
            for _ in 0..3 {
                crate::width_walk::frame(cx);
            }
        };
        // The stacked list takes half the body drawn, up to its 324 px.
        let half = |cx: &mut VisualTestContext, at: &str| {
            let body = cx.debug_bounds("github-panel").expect("the body");
            let list = cx.debug_bounds("github-file-focus").expect("the file list");
            assert!(
                (list.size.height - px(324.).min(body.size.height / 2.)).abs() <= slack,
                "{at}: the list {list:?} takes half the body {body:?}"
            );
            list
        };
        let pinned = |cx: &mut VisualTestContext| {
            cx.read(|cx| {
                let panel = panel.read(cx);
                (
                    panel.conversations.active.is_some(),
                    panel.review.top_line_pinned.get(),
                    panel.review.top_line.get(),
                )
            })
        };
        for interface in [13, 18] {
            let at = format!("{interface} pt in 461 × 490");
            show_overview(&panel, &pull, (interface, 461., 490.), cx);
            let files = files.clone();
            cx.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    let target = panel.conversations.threads[0].target.clone();
                    panel.begin_reply(target, window, cx);
                    // After the reply opens, which clears a stale notice.
                    panel.notice = Some("Fixture PRs refreshed.".into());
                    // As a file page lands, no file selected.
                    panel.review.files = files;
                    panel.review.selected_file = None;
                    panel.change_section(Section::Files, window, cx);
                })
            });
            settle_panel(&app, cx).await;
            // The frames the change drew, before any the test draws.
            let unselected = format!("{at}, no file selected");
            half(cx, &unselected);
            for _ in 0..4 {
                crate::width_walk::frame(cx);
            }
            let state = pinned(cx);
            assert!(
                state.0 && state.1 && state.2 > px(0.),
                "{at}: the reply stays open in Files, its top line above the body: {state:?}"
            );
            list_at_top(cx);
            let before = half(cx, &unselected);
            let row = cx.debug_bounds("github-file-row-0").expect("a file row");
            cx.simulate_click(row.center(), Modifiers::default());
            settle_panel(&app, cx).await;
            let clicked = cx.debug_bounds("github-file-focus").expect("the file list");
            assert!(
                (clicked.size.height - before.size.height).abs() <= slack,
                "{at}: a click on a file leaves the list {before:?} as tall: {clicked:?}"
            );
            for _ in 0..4 {
                crate::width_walk::frame(cx);
            }
            assert_eq!(
                cx.read(|cx| panel.read(cx).review.selected_file),
                Some(0),
                "{at}: the click selects the file"
            );
            list_at_top(cx);
            let body = cx.debug_bounds("github-panel").expect("the body");
            let list = cx.debug_bounds("github-file-focus").expect("the file list");
            let content = cx.debug_bounds("github-patch").expect("the file's box");
            let clip = cx.debug_bounds("github-patch-rows").expect("the rows");
            let row = cx.debug_bounds("github-patch-line-0").expect("a patch row");
            assert!(
                list.size.height <= body.size.height / 2. + slack,
                "{at}: the list {list:?} takes at most half the body {body:?}"
            );
            let five = clip.size.height < row.size.height * 6.;
            assert!(
                (content.bottom() - (body.bottom() - room)).abs() <= px(1.)
                    || (five && content.bottom() > body.bottom() - room),
                "{at}: the file's box {content:?} ends at the body's foot {body:?}, less the \
                 ring's room, or shows its five lines past it"
            );
            half(cx, &format!("{at}, a file selected"));
            // A longer notice, the line taller: the list follows in the frames it draws.
            cx.update(|_, cx| {
                panel.update(cx, |panel, cx| {
                    panel.notice = Some(
                        "Fixture PRs refreshed. GitHub.com is supported; Git authentication and \
                         Push keep their existing behavior."
                            .into(),
                    );
                    cx.notify();
                })
            });
            settle_panel(&app, cx).await;
            half(cx, &format!("{at}, a longer notice"));
            // The notice clears with no file selected and the conversation still open: the line
            // takes no height, and only Files' check of the height the line drew redraws the
            // list, so the frames the change draws settle it, before any the test draws.
            cx.update(|_, cx| {
                panel.update(cx, |panel, cx| {
                    panel.review.selected_file = None;
                    cx.notify();
                })
            });
            settle_panel(&app, cx).await;
            cx.update(|_, cx| {
                panel.update(cx, |panel, cx| {
                    panel.notice = None;
                    cx.notify();
                })
            });
            settle_panel(&app, cx).await;
            half(cx, &format!("{at}, the notice cleared"));
            // The conversation closes with no file selected, the line among the content.
            cx.update(|_, cx| {
                panel.update(cx, |panel, cx| {
                    panel.review.selected_file = None;
                    panel.conversations.active = None;
                    cx.notify();
                })
            });
            settle_panel(&app, cx).await;
            let closed = format!("{at}, the conversation closed");
            half(cx, &closed);
            for _ in 0..4 {
                crate::width_walk::frame(cx);
            }
            let state = pinned(cx);
            assert!(
                !state.1,
                "{closed}: the top line among the content: {state:?}"
            );
            half(cx, &closed);
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Files settles in one frame after a resize or a text size change and then holds still,
    /// from side by side to stacked and back, at 13 and 18 pt: the frame drawn at the new size
    /// lays it out from the window, and only the height of the file's title and mode row, which
    /// wrap anew at the new width, can ask for one more ([`review::FilesFit`]). Measured from
    /// the last frame, the columns' width and the body's height took a frame to follow a
    /// resize, and the height above the box, measured in the layout they replaced, another.
    #[gpui::test]
    async fn files_settles_in_a_frame_after_a_resize(cx: &mut TestAppContext) {
        let (_fixture, app, panel, pull, cx) = fixture_panel(cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        let captured = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let files = Client(UiTransport::Fixture { moved: false })
            .files(&captured, 1, &OperationControl::default())
            .unwrap()
            .items;
        crate::focus_ring::sweep::interface_size(cx, 13);
        cx.simulate_resize(size(px(1480.), px(800.)));
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.pull = Some(pull.clone());
                panel.review.show_list = false;
                panel.review.section = Section::Files;
                panel.review.files = files.clone();
                panel.review.selected_file = None;
                panel.select_review_file(0, window, cx);
            })
        });
        settle_panel(&app, cx).await;
        let redraws = || review::FILES_FIT_REDRAWS.with(|redraws| redraws.replace(0));
        let steps: [(u8, f32, f32); 7] = [
            (13, 461., 490.),
            (13, 1480., 800.),
            (18, 1480., 800.),
            (18, 461., 490.),
            (18, 500., 490.),
            (13, 500., 490.),
            (13, 1480., 800.),
        ];
        for (interface, width, height) in steps {
            let at = format!("{interface} pt in {width} × {height}");
            redraws();
            crate::focus_ring::sweep::interface_size(cx, interface);
            settle_panel(&app, cx).await;
            let after_text = redraws();
            cx.simulate_resize(size(px(width), px(height)));
            settle_panel(&app, cx).await;
            for _ in 0..3 {
                crate::width_walk::frame(cx);
            }
            let after_resize = redraws();
            assert!(
                after_text <= 1 && after_resize <= 1,
                "{at}: Files drew {after_text} more frames after the text size changed and \
                 {after_resize} after the resize"
            );
            let list = cx.debug_bounds("github-file-focus").expect("the file list");
            let column = cx.debug_bounds("github-file-column").expect("the file");
            assert!(
                if width > 1000. {
                    column.left() > list.right()
                } else {
                    column.top() > list.bottom()
                },
                "{at}: the file {column:?} beside the list {list:?} in a wide window, below it \
                 in a narrow one"
            );
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Opens Files on the fixture's first page at 13 pt in a 1,480 × 800 window, its first file
    /// selected and prepared.
    async fn show_first_file(
        app: &Entity<GitTurtle>,
        panel: &Entity<Panel>,
        pull: &PullRequest,
        cx: &mut VisualTestContext,
    ) {
        let captured = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let files = Client(UiTransport::Fixture { moved: false })
            .files(&captured, 1, &OperationControl::default())
            .unwrap()
            .items;
        crate::focus_ring::sweep::interface_size(cx, 13);
        cx.simulate_resize(size(px(1480.), px(800.)));
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.pull = Some(pull.clone());
                panel.review.show_list = false;
                panel.review.section = Section::Files;
                panel.review.files = files;
                panel.review.selected_file = None;
                panel.select_review_file(0, window, cx);
            })
        });
        settle_panel(app, cx).await;
    }

    /// Whether the last frame stood the file beside the changed-file list rather than below it.
    fn files_beside(cx: &mut VisualTestContext) -> bool {
        let list = cx.debug_bounds("github-file-focus").expect("the file list");
        let column = cx.debug_bounds("github-file-column").expect("the file");
        column.left() > list.right()
    }

    /// The narrowest window 800 px tall, to `step`, in which Files stands the file beside the
    /// list, between 400 px, where it stands below, and 1,480 px.
    fn narrowest_beside(cx: &mut VisualTestContext, step: f32) -> f32 {
        let beside_at = |cx: &mut VisualTestContext, width: f32| {
            cx.simulate_resize(size(px(width), px(800.)));
            for _ in 0..3 {
                crate::width_walk::frame(cx);
            }
            files_beside(cx)
        };
        let (mut below, mut beside) = (400., 1480.);
        assert!(
            !beside_at(cx, below) && beside_at(cx, beside),
            "the file stands below the list in a 400 px window and beside it in a 1,480 px one"
        );
        while beside - below > step {
            let middle = below + ((beside - below) / step / 2.).floor() * step;
            if beside_at(cx, middle) {
                beside = middle;
            } else {
                below = middle;
            }
        }
        beside
    }

    /// Where the last frame stood the file beside the list, its mode row, Changes, Source,
    /// Threads and the comment Button, lies on one line inside the file's column.
    fn assert_mode_row_on_one_line(cx: &mut VisualTestContext, at: &str) {
        if !files_beside(cx) {
            return;
        }
        let column = cx.debug_bounds("github-file-column").expect("the file");
        let first = cx.debug_bounds("github-file-mode-0").expect("Changes");
        let last = cx
            .debug_bounds("github-inline-start")
            .expect("the comment Button");
        assert!(
            (last.top() - first.top()).abs() <= px(0.5) && last.right() <= column.right() + px(0.5),
            "{at}: beside the list, the mode row from {first:?} to {last:?} keeps one line in \
             the file's column {column:?}"
        );
    }

    /// Files stands the file beside the list where its mode row fits there on one line with
    /// the label its last Button reads now, at 13 pt (`review::mode_row_width`): in the window
    /// a pixel too narrow for Comment on selection beside the list, an open comment, whose
    /// Button reads the narrower Continue comment, stands the file beside the list, its row on
    /// one line, and as the comment closes the file stands below the list again, each in the
    /// frames the change draws. The row's width measured with one label and kept for the other
    /// would keep the other's layout.
    #[gpui::test]
    async fn files_mode_row_follows_its_last_label(cx: &mut TestAppContext) {
        let (_fixture, app, panel, pull, cx) = fixture_panel(cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        show_first_file(&app, &panel, &pull, cx).await;
        let beside = narrowest_beside(cx, 1.);
        let wide = cx
            .debug_bounds("github-inline-start")
            .expect("Comment on selection");
        let width = beside - 1.;
        cx.simulate_resize(size(px(width), px(800.)));
        for _ in 0..3 {
            crate::width_walk::frame(cx);
        }
        assert!(!files_beside(cx), "the file below the list at {width} px");
        let comment = cx.read(|cx| {
            let file = &panel.read(cx).review.files[0];
            let row = file
                .rows
                .iter()
                .position(|row| row.kind == b'+')
                .expect("an added line");
            file.selection(row, row).expect("an inline position")
        });
        for open in [true, false, true, false] {
            let label = if open {
                "Continue comment"
            } else {
                "Comment on selection"
            };
            let at = format!("{label} in a {width} px window");
            let comment = comment.clone();
            cx.update(|_, cx| {
                panel.update(cx, |panel, cx| {
                    panel.review.composing = open.then_some(comment);
                    cx.notify();
                })
            });
            assert_eq!(
                files_beside(cx),
                open,
                "{at}: the file beside the list only with the narrower label"
            );
            for _ in 0..2 {
                crate::width_walk::frame(cx);
            }
            assert_eq!(files_beside(cx), open, "{at}, two frames on");
            assert_mode_row_on_one_line(cx, &at);
            let button = cx.debug_bounds("github-inline-start").expect(label);
            assert!(
                !open || button.size.width < wide.size.width - px(1.),
                "{at}: the Button {button:?} is narrower than Comment on selection {wide:?} by \
                 more than the pixel"
            );
        }
    }

    /// Files measures its mode row again as the display's scale changes between frames, at
    /// 13 pt (`review::mode_row_width`): in the narrowest window at 2x in which the file stands
    /// beside the list, to an eighth of a pixel, a scale at which the row's Buttons, each laid
    /// out on whole device pixels, take more room than the file's column leaves stands the
    /// file below the list, wherever the file stands beside the list its row keeps one line in
    /// the column, and back at 2x it stands beside the list again. The row's width measured at
    /// another scale and kept stood the file beside the list with its row wrapped.
    #[gpui::test]
    async fn files_mode_row_follows_the_display_scale(cx: &mut TestAppContext) {
        let (_fixture, app, panel, pull, cx) = fixture_panel(cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        show_first_file(&app, &panel, &pull, cx).await;
        let width = narrowest_beside(cx, 0.125);
        cx.simulate_resize(size(px(width), px(800.)));
        for _ in 0..3 {
            crate::width_walk::frame(cx);
        }
        let mut below = Vec::new();
        for scale in [1., 1.25, 1.5, 1.75] {
            cx.update(|window, _| window.set_scale_factor(scale));
            for _ in 0..3 {
                crate::width_walk::frame(cx);
            }
            assert_mode_row_on_one_line(cx, &format!("at {scale}x in a {width} px window"));
            if !files_beside(cx) {
                below.push(scale);
            }
            cx.update(|window, _| window.set_scale_factor(2.));
            for _ in 0..3 {
                crate::width_walk::frame(cx);
            }
            let at = format!("back at 2x from {scale}x in a {width} px window");
            assert!(files_beside(cx), "{at}: the file beside the list");
            assert_mode_row_on_one_line(cx, &at);
        }
        assert!(
            !below.is_empty(),
            "a scale's device pixels widen the row past the column at {width} px"
        );
    }

    /// Shift+Tab back onto a conversation's last action reveals it again in the conversation
    /// list, at 13 pt in a 470 × 490 window, after Tab took focus on to the conversations'
    /// pager and the list scrolled back to its top: the list kept the action as the focus it
    /// had shown, so the reveal saw no change of focus, and the action stayed out of view with
    /// its ring undrawn (FU18-F's sweep, Overview's stop 12).
    #[gpui::test]
    async fn a_conversation_action_focused_again_is_revealed_again(cx: &mut TestAppContext) {
        let (_fixture, app, panel, pull, cx) = fixture_panel(cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        show_overview(&panel, &pull, (13, 470., 490.), cx);
        let contexts = |cx: &mut VisualTestContext| {
            cx.update(|window, _| {
                window
                    .context_stack()
                    .iter()
                    .map(|context| format!("{context:?}"))
                    .collect::<Vec<_>>()
                    .join(" < ")
            })
        };
        let list = cx.read(|cx| panel.read(cx).conversations.list_scroll.clone());
        let mut last = String::new();
        for _ in 0..20 {
            cx.simulate_keystrokes("tab");
            for _ in 0..3 {
                crate::width_walk::frame(cx);
            }
            let now = contexts(cx);
            if now.contains("conversations-pager") {
                break;
            }
            last = now;
        }
        assert!(
            last.contains("GitTurtleGithubConversation=2") && list.offset().y < px(0.),
            "Tab reached the last conversation's last action, the list scrolled to it: {last}"
        );
        list.set_offset(point(px(0.), px(0.)));
        for _ in 0..3 {
            crate::width_walk::frame(cx);
        }
        cx.simulate_keystrokes("shift-tab");
        for _ in 0..3 {
            crate::width_walk::frame(cx);
        }
        assert_eq!(contexts(cx), last, "Shift+Tab returns to the action");
        let viewport = cx.debug_bounds("github-threads").expect("the list");
        let actions = cx
            .debug_bounds("github-thread-actions-2")
            .expect("the actions");
        assert!(
            actions.top() >= viewport.top() && actions.bottom() <= viewport.bottom(),
            "the list shows the actions {actions:?} again in {viewport:?}"
        );
    }

    /// Shift+Tab from Back to repository, below the body, back onto the body's last control
    /// reveals it again with its ring, at 13 pt in a 470 × 490 window, after the wheel scrolled
    /// the body back to its top: the body kept the control as the focus it had shown, so the
    /// reveal saw no change of focus, and the control stayed below the body's foot with its
    /// ring undrawn (coordinator's decision, 2026-11-05: a control focus returns to is revealed
    /// again wherever its list has scrolled since).
    #[gpui::test]
    async fn the_bodys_last_control_focused_again_is_revealed_again(cx: &mut TestAppContext) {
        let (_fixture, app, panel, pull, cx) = fixture_panel(cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        show_overview(&panel, &pull, (13, 470., 490.), cx);
        let scroll = cx.read(|cx| panel.read(cx).review.body_scroll.clone());
        let stop = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                let inside = window
                    .context_stack()
                    .iter()
                    .any(|context| context.contains(review::BODY));
                (window.focused(cx), inside)
            })
        };
        // The Button rings the last frame drew whole inside the body.
        let rings = |cx: &mut VisualTestContext| {
            let body = cx.debug_bounds("github-panel").expect("the body");
            let (ring, color, scale) = cx.update(|window, cx| {
                let theme = gpui_kit::component::Theme::global(cx);
                let ring = theme.button_focus_ring;
                (ring, theme.ring.alpha(ring.opacity), window.scale_factor())
            });
            quads(cx)
                .into_iter()
                .filter(|(bounds, quad)| {
                    quad.background.is_transparent()
                        && quad.border_color == color
                        && quad.border_widths.top.as_f32() == ring.width.as_f32() * scale
                        && bounds.top() >= body.top()
                        && bounds.bottom() <= body.bottom()
                })
                .map(|(bounds, _)| bounds)
                .collect::<Vec<_>>()
        };
        let mut last = None;
        for _ in 0..30 {
            cx.simulate_keystrokes("tab");
            for _ in 0..3 {
                crate::width_walk::frame(cx);
            }
            let (focused, inside) = stop(cx);
            if !inside && last.is_some() {
                break;
            }
            last = inside.then(|| (focused, scroll.offset().y, rings(cx)));
        }
        let Some((control, shown, ringed)) = last else {
            panic!("Tab reached a control in the body before one below it");
        };
        assert!(
            shown < px(0.) && !ringed.is_empty(),
            "Tab scrolled the body {shown:?} onto its last control, its ring drawn {ringed:?}"
        );
        assert!(
            rings(cx).is_empty(),
            "Tab moved on to Back to repository, below the body"
        );
        // The wheel, over the body's top corner, clear of the conversation list, takes the body
        // back to its top, the control out of view.
        let body = cx.debug_bounds("github-panel").expect("the body");
        cx.simulate_event(ScrollWheelEvent {
            position: body.origin + point(px(2.), px(2.)),
            delta: ScrollDelta::Pixels(point(px(0.), -shown + px(100.))),
            modifiers: Modifiers::default(),
            touch_phase: TouchPhase::Moved,
        });
        for _ in 0..3 {
            crate::width_walk::frame(cx);
        }
        assert_eq!(
            scroll.offset().y,
            px(0.),
            "the wheel scrolls the body to its top"
        );
        cx.simulate_keystrokes("shift-tab");
        for _ in 0..3 {
            crate::width_walk::frame(cx);
        }
        assert!(
            stop(cx) == (control, true),
            "Shift+Tab returns to the control"
        );
        let again = rings(cx);
        assert!(
            scroll.offset().y < px(0.) && !again.is_empty(),
            "the body {:?} shows the control again with its ring {again:?}",
            scroll.offset().y
        );
    }

    /// What Files' lists and clips hide is no line of the body, at 13 and 18 pt in a 461 × 490
    /// window: the changed-file list, holding twelve files, scrolled half a row inside itself
    /// and standing past the body's top edge, and Source's text, standing past the body's
    /// foot, each tell the body their viewport ([`review::clipped`]), so the row the list's own
    /// edge cuts, past that edge, and the part of a row under Source's clip, inside the
    /// editor's bottom padding, are none of its lines: no line of the body crosses either
    /// edge of either. Past the body's edge, where the clip around them is the body's, the
    /// body once took such a row for a line, and a reveal could refuse a stop that cut nothing
    /// a person sees (FU18-F's review).
    #[gpui::test]
    async fn rows_files_hides_past_its_edges_are_no_lines_of_the_body(cx: &mut TestAppContext) {
        let (_fixture, app, panel, pull, cx) = fixture_panel(cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        let captured = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let listed = Client(UiTransport::Fixture { moved: false })
            .files(&captured, 1, &OperationControl::default())
            .unwrap()
            .items;
        // The fixture's files, and copies of them under other names, so the list scrolls.
        let files: Vec<_> = (0..12)
            .map(|index| {
                let mut file = listed[index % listed.len()].clone();
                if index >= listed.len() {
                    file.filename = format!("copy-{index}/{}", file.filename);
                }
                file
            })
            .collect();
        let scroll = cx.read(|cx| panel.read(cx).review.body_scroll.clone());
        let slack = px(0.5);
        // Scrolls the body so `edge`, a place in the frame, lies at `at`.
        let place = |edge: Pixels, at: Pixels, cx: &mut VisualTestContext| {
            let offset = (scroll.offset().y + at - edge).clamp(-scroll.max_offset().y, px(0.));
            scroll.set_offset(point(px(0.), offset));
            for _ in 0..3 {
                crate::width_walk::frame(cx);
            }
        };
        // The lines of the body that cross the top or bottom edge of the clip `name` draws and
        // that `ours` takes for one of its rows, the text beside the clip apart.
        let crossing = |name: &'static str,
                        ours: &dyn Fn(&(Pixels, Pixels)) -> bool,
                        cx: &mut VisualTestContext| {
            let clip = cx.debug_bounds(name).expect("the clip");
            let lines = panel.read_with(cx, |panel, cx| panel.review.body_blocks.lines(cx));
            let crosses = |edge: Pixels, (top, bottom): &(Pixels, Pixels)| {
                *top < edge - slack && *bottom > edge + slack
            };
            lines
                .into_iter()
                .filter(|line| crosses(clip.top(), line) || crosses(clip.bottom(), line))
                .filter(|line| ours(line))
                .collect::<Vec<_>>()
        };
        for interface in [13, 18] {
            let at = format!("{interface} pt in 461 × 490");
            crate::focus_ring::sweep::interface_size(cx, interface);
            cx.simulate_resize(size(px(461.), px(490.)));
            let (pull, files) = (pull.clone(), files.clone());
            cx.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    panel.pull = Some(pull);
                    panel.review.show_list = false;
                    panel.review.section = Section::Files;
                    panel.review.files = files;
                    panel.review.selected_file = None;
                    panel.select_review_file(0, window, cx);
                })
            });
            settle_panel(&app, cx).await;
            for _ in 0..3 {
                crate::width_walk::frame(cx);
            }
            // The list scrolled half a row inside itself, its top edge 10 px above the body's.
            let row = cx.debug_bounds("github-file-row-0").expect("a file row");
            cx.update(|_, cx| {
                let panel = panel.read(cx);
                let handle = panel.review.file_scroll.0.borrow();
                handle
                    .base_handle
                    .set_offset(point(px(0.), -row.size.height / 2.));
            });
            for _ in 0..3 {
                crate::width_walk::frame(cx);
            }
            let list = cx.debug_bounds("github-file-rows").expect("the list");
            let body = cx.debug_bounds("github-panel").expect("the body");
            place(list.top(), body.top() - px(10.), cx);
            let list = cx.debug_bounds("github-file-rows").expect("the list");
            let body = cx.debug_bounds("github-panel").expect("the body");
            let rows: Vec<Bounds<Pixels>> = indexed("github-file-row-", 12)
                .into_iter()
                .filter_map(|name| cx.debug_bounds(name))
                .collect();
            let cut = rows
                .iter()
                .any(|row| row.top() < list.top() - slack && row.bottom() > list.top() + slack);
            assert!(
                cut && list.top() < body.top() - slack && list.bottom() > body.top(),
                "{at}: the list {list:?} cuts a row at its top, past the body's {body:?}"
            );
            let a_row = |(top, bottom): &(Pixels, Pixels)| {
                rows.iter().any(|row| {
                    (row.top() - *top).abs() < px(0.01) && (row.bottom() - *bottom).abs() < px(0.01)
                })
            };
            assert_eq!(
                crossing("github-file-rows", &a_row, cx),
                Vec::new(),
                "{at}: no line of the body crosses the file list's edges"
            );
            // Source's text, its clip's foot 10 px below the body's.
            cx.update(|window, cx| {
                panel.update(cx, |panel, cx| panel.review_source(1, window, cx))
            });
            settle_panel(&app, cx).await;
            for _ in 0..3 {
                crate::width_walk::frame(cx);
            }
            let clip = cx
                .debug_bounds("github-source-text")
                .expect("Source's clip");
            let body = cx.debug_bounds("github-panel").expect("the body");
            place(clip.bottom(), body.bottom() + px(10.), cx);
            let clip = cx
                .debug_bounds("github-source-text")
                .expect("Source's clip");
            let body = cx.debug_bounds("github-panel").expect("the body");
            let (view, line) = cx.read(|cx| {
                let source = panel.read(cx).review.source.clone().expect("Source");
                let source = source.read(cx);
                (
                    source.input_bounds(),
                    source.line_height().expect("Source has painted"),
                )
            });
            assert!(
                clip.bottom() > body.bottom() + slack
                    && clip.top() < body.bottom()
                    && view.bottom() >= clip.bottom() - slack,
                "{at}: Source's clip {clip:?} stands past the body's foot {body:?}, the editor \
                 {view:?} to its foot"
            );
            assert_eq!(
                crossing(
                    "github-source-text",
                    &|(top, bottom)| *top >= clip.top() - slack && *bottom - *top <= line + slack,
                    cx
                ),
                Vec::new(),
                "{at}: no line of the body crosses Source's clip"
            );
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// A surface of the panel the reveal sweep walks ([`sweep_reveals`]) on the offline
    /// fixture: the connection pane above the PR list, the PR list, a PR's Overview with its
    /// Conversations pane, its Review, and Files in Changes, Source and Threads, the file's
    /// Conversations pane.
    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Surface {
        Connection,
        Pulls,
        Overview,
        Review,
        Changes,
        Source,
        Threads,
    }

    /// Every surface the reveal sweep walks.
    const SURFACES: [Surface; 7] = [
        Surface::Connection,
        Surface::Pulls,
        Surface::Overview,
        Surface::Review,
        Surface::Changes,
        Surface::Source,
        Surface::Threads,
    ];

    /// Surfaces the reveal sweep walks at one size, an interface size and a window's width and
    /// height ([`sweep_reveals`]).
    type Walk<'a> = ((u8, f32, f32), &'a [Surface]);

    /// The keys the reveal sweep walks each surface with, in turn ([`sweep_reveals`]): Tab
    /// through every stop, then Shift+Tab, which reveals each from the other side.
    const BOTH_WAYS: [&str; 2] = ["tab", "shift-tab"];

    /// What the reveal sweep found ([`sweep_reveals`]), each finding named by its surface,
    /// size and stop.
    #[derive(Default)]
    struct Swept {
        /// Reveals that left a line cut at an edge of the body where a place clears every line
        /// with the focused control whole with its ring.
        cuts: Vec<String>,
        /// Stops that show no whole ring (`focus_ring::sweep::walk_each`), but for a control
        /// too tall for the body with its ring's room, which shows the ring's top edge.
        rings: Vec<String>,
        /// Lines outside the lists inside the body that no stop of a surface's walks shows
        /// whole, and texts and Buttons part of which no stop shows at all.
        unseen: Vec<String>,
        /// How many stops the walks took, how many of them moved the body, and how many of
        /// those revealed a part too tall for the body, whose cuts the sweep leaves alone.
        stops: usize,
        moves: usize,
        tall: usize,
    }

    /// Shows `surface` on the offline fixture at `interface` pt in a `width` × `height`
    /// window, with `files` listed in Files and the first selected, the body and the
    /// conversation list at their tops and focus where the surface opens it.
    async fn show_surface(
        (app, panel, pull, files): (
            &Entity<GitTurtle>,
            &Entity<Panel>,
            &PullRequest,
            &[crate::github::review::PullFile],
        ),
        surface: Surface,
        (interface, width, height): (u8, f32, f32),
        cx: &mut VisualTestContext,
    ) {
        crate::focus_ring::sweep::interface_size(cx, interface);
        cx.simulate_resize(size(px(width), px(height)));
        let (pull, files) = (pull.clone(), files.to_vec());
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                let listed = matches!(surface, Surface::Connection | Surface::Pulls);
                panel.review.connection_open = surface == Surface::Connection;
                panel.pull = (!listed).then_some(pull);
                panel.review.show_list = listed;
                panel.review.section = match surface {
                    Surface::Review => Section::Review,
                    Surface::Changes | Surface::Source | Surface::Threads => Section::Files,
                    _ => Section::Overview,
                };
                panel.review.files = files;
                panel.review.selected_file = None;
                panel.select_review_file(0, window, cx);
            })
        });
        settle_panel(app, cx).await;
        let mode = match surface {
            Surface::Source => 1,
            Surface::Threads => 2,
            _ => 0,
        };
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                if mode > 0 {
                    panel.review_source(mode, window, cx);
                }
                panel.review.body_scroll.set_offset(point(px(0.), px(0.)));
                panel
                    .conversations
                    .list_scroll
                    .set_offset(point(px(0.), px(0.)));
                panel.focus_visible_section(window, cx);
                cx.notify();
            })
        });
        settle_panel(app, cx).await;
        for _ in 0..3 {
            crate::width_walk::frame(cx);
        }
    }

    /// Walks each of `walks`' surfaces on the offline fixture at its size (an interface size
    /// and a window's width and height), through every stop with each of `keys` in turn, Tab
    /// and Shift+Tab ([`BOTH_WAYS`]) or one of them, each walk from the surface opened at the
    /// body's top (`focus_ring::sweep::walk_each`), and returns what it found ([`Swept`]):
    /// every stop's ring that is not whole; every reveal that left a line drawn in the frame
    /// ([`frame_lines`]) cut at an edge of the body where a place the body's top edge can take
    /// keeps the focused control whole with its ring and, drawn there, cuts no line
    /// ([`needless_cut`]), but at the content's top, which a reveal keeps
    /// ([`review::clear_of_lines`]); and every line outside the lists inside the body that no
    /// stop of the surface's walks shows whole, with every text and Button outside them part
    /// of which no stop shows at all.
    async fn sweep_reveals(cx: &mut TestAppContext, walks: &[Walk<'_>], keys: &[&str]) -> Swept {
        use crate::native_accessibility::written;
        let (_fixture, app, panel, pull, cx) = fixture_panel(cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        let captured = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let files = Client(UiTransport::Fixture { moved: false })
            .files(&captured, 1, &OperationControl::default())
            .unwrap()
            .items;
        let scroll = cx.read(|cx| panel.read(cx).review.body_scroll.clone());
        let room = cx.update(|_, cx| appearance::button_ring_room(cx));
        let slack = px(0.5);
        let mut swept = Swept::default();
        for &((interface, width, height), surfaces) in walks {
            for &surface in surfaces {
                let at = format!("{surface:?} at {interface} pt in {width} × {height}");
                // Each line outside the lists by its place, and whether a stop showed it whole;
                // each text and Button outside them by its place and extent in the content; and
                // the extent of the content each stop showed.
                let mut lines: std::collections::BTreeMap<(i32, i32), (String, bool)> =
                    Default::default();
                let mut boxes: std::collections::BTreeMap<(i32, i32), (String, Pixels, Pixels)> =
                    Default::default();
                let mut views: Vec<(Pixels, Pixels)> = Vec::new();
                for &key in keys {
                    show_surface(
                        (&app, &panel, &pull, &files),
                        surface,
                        (interface, width, height),
                        cx,
                    )
                    .await;
                    let mut before = scroll.offset().y;
                    let mut each =
                        |cx: &mut VisualTestContext,
                         stop: &str,
                         ring: &[crate::focus_ring::PaintedOutline]| {
                            let now = scroll.offset().y;
                            let (body, drawn) = frame_lines(cx, &panel);
                            swept.stops += 1;
                            let origin = body.top() + now;
                            views.push((body.top() - origin, body.bottom() - origin));
                            for line in drawn.iter().filter(|line| !line.nested) {
                                let whole = line.top >= body.top() - slack
                                    && line.bottom <= body.bottom() + slack;
                                let seen = lines
                                    .entry(line.place)
                                    .or_insert_with(|| (line.name.clone(), false));
                                seen.1 |= whole;
                            }
                            let lists: Vec<Bounds<Pixels>> = BODY_LISTS
                                .iter()
                                .filter_map(|list| cx.debug_bounds(list))
                                .collect();
                            let scale = cx.update(|window, _| window.scale_factor());
                            // What a list inside the body holds is the list's to show, where
                            // its own scroll has taken it too: the conversation list's rows
                            // below its foot once counted as text no stop of the body showed.
                            let listed: Vec<String> = BODY_LISTS
                                .iter()
                                .flat_map(|list| written::inside(list))
                                .collect();
                            for id in written::inside("github-panel") {
                                if listed.contains(&id) {
                                    continue;
                                }
                                let Some(text) = written::get(&id) else {
                                    continue;
                                };
                                let (Role::Label | Role::Button, Some(layout)) =
                                    (text.node.role(), text.layout)
                                else {
                                    continue;
                                };
                                let bounds = Bounds::from_corners(
                                    point(
                                        px(layout.x0 as f32 / scale),
                                        px(layout.y0 as f32 / scale),
                                    ),
                                    point(
                                        px(layout.x1 as f32 / scale),
                                        px(layout.y1 as f32 / scale),
                                    ),
                                );
                                if bounds.size.height <= px(0.)
                                    || lists.iter().any(|list| list.intersects(&bounds))
                                {
                                    continue;
                                }
                                let half = |value: Pixels| (f32::from(value) * 2.).round() as i32;
                                boxes
                                    .entry((half(bounds.left()), half(bounds.top() - origin)))
                                    .or_insert((
                                        id,
                                        bounds.top() - origin,
                                        bounds.bottom() - origin,
                                    ));
                            }
                            // A ring a clip cuts: a control that cannot fit the body with its ring's
                            // room shows its top, the ring's top edge drawn, as the Conversations
                            // pane does; any other ring is whole.
                            for outline in ring.iter().filter(|outline| !outline.whole) {
                                let tall = outline.bounds.size.height + room * 2.
                                    > body.size.height + slack;
                                if !tall || !outline.sides[0] {
                                    swept.rings.push(format!(
                                    "{stop}: a clip cuts its ring {:?}, sides whole {:?}, in the \
                                     body {body:?}",
                                    outline.bounds, outline.sides
                                ));
                                }
                            }
                            let moved = now != before;
                            before = now;
                            if !moved {
                                return;
                            }
                            swept.moves += 1;
                            if now == px(0.) {
                                return;
                            }
                            match needless_cut(cx, &panel, (body, &drawn), ring, stop) {
                                Err(()) => swept.tall += 1,
                                Ok(Some(finding)) => swept.cuts.push(finding),
                                Ok(None) => {}
                            }
                        };
                    // The walk's own findings but its clipped rings, which `each` reads above.
                    let mut walked = Vec::new();
                    crate::focus_ring::sweep::walk_each(
                        &app,
                        cx,
                        key,
                        &at,
                        interface,
                        &mut walked,
                        &mut each,
                    );
                    swept.rings.extend(
                        walked
                            .into_iter()
                            .filter(|failure| !failure.contains(": a clip cuts the ")),
                    );
                }
                swept.unseen.extend(
                    lines
                        .iter()
                        .filter(|(_, (_, whole))| !whole)
                        .map(|(place, (name, _))| {
                            format!("{at}: {name} at {place:?} is never whole")
                        }),
                );
                // What of each text and Button no stop showed at all.
                views.sort_by(|a, b| a.0.as_f32().total_cmp(&b.0.as_f32()));
                for (name, top, bottom) in boxes.values() {
                    let mut reach = *top;
                    for (shown, ends) in &views {
                        if *shown > reach + slack {
                            break;
                        }
                        reach = reach.max(*ends);
                    }
                    if reach < *bottom - slack {
                        swept.unseen.push(format!(
                            "{at}: no stop shows {name} {top:?}..{bottom:?} below {reach:?}"
                        ));
                    }
                }
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        swept
    }

    /// At 18 pt in a 461 × 490 window, every stop of the PR list, the connection pane above it,
    /// Changes and Source, walked with Tab and with Shift+Tab from the surface opened at the
    /// body's top, draws its ring whole inside every clip around it (`focus_ring::sweep`), but
    /// for the connection pane's own stop, which is taller than the body there and draws its
    /// ring on its edge, so its reveal shows its top, the ring's top edge whole. The
    /// reveal sweep found two cut ([`sweep_reveals`]): Tab onto the PR list's first row left
    /// its ring's top edge half a pixel above the body's top, where the reveal's stop clear of
    /// every line was allowed half a pixel inside the part it shows; and Tab onto Changes'
    /// patch, which fits the body with its ring's room, revealed the file's part, which is
    /// taller than the body, at its top, the patch's foot and its ring below the body's.
    #[gpui::test]
    async fn reveals_draw_the_pr_rows_and_the_files_boxes_rings_whole(cx: &mut TestAppContext) {
        use crate::focus_ring::sweep;
        let (_fixture, app, panel, pull, cx) = fixture_panel(cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        let captured = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let files = Client(UiTransport::Fixture { moved: false })
            .files(&captured, 1, &OperationControl::default())
            .unwrap()
            .items;
        let mut failures = Vec::new();
        for surface in [
            Surface::Connection,
            Surface::Pulls,
            Surface::Changes,
            Surface::Source,
        ] {
            for key in ["tab", "shift-tab"] {
                show_surface((&app, &panel, &pull, &files), surface, (18, 461., 490.), cx).await;
                sweep::walk_each(
                    &app,
                    cx,
                    key,
                    &format!("{surface:?} by {key}"),
                    18,
                    &mut failures,
                    &mut |_, _, _| {},
                );
            }
        }
        sweep::assert_whole_except(
            failures,
            &[
                (
                    "Connection by tab at 18 pt, stop 2",
                    "a clip cuts the right, bottom, left",
                ),
                (
                    "Connection by shift-tab at 18 pt, stop 5",
                    "a clip cuts the right, bottom, left",
                ),
            ],
        );
    }

    /// Tab onto the PR list's row from Account options, and Shift+Tab onto it from Recover
    /// drafts, at 17 pt in a 461 × 490 window, leave no line cut at the body's edges, the
    /// title whole with its ring and its row's context line with it: the title is a part of
    /// its own. Revealed as the list, the row's reveal kept the room the list keeps below its
    /// last row, which no place with every line whole showed, and left Previous PR page cut
    /// at the body's foot (Tab, the body's top edge could stop clear of every line 43.5 px
    /// higher) or the second line of Drafts stay on this computer (Shift+Tab, 199 px higher),
    /// as FU27-B's probe found.
    #[gpui::test]
    async fn a_pr_rows_reveal_leaves_no_line_cut_at_17_pt(cx: &mut TestAppContext) {
        let (_fixture, app, panel, pull, cx) = fixture_panel(cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        for key in BOTH_WAYS {
            show_surface(
                (&app, &panel, &pull, &[]),
                Surface::Pulls,
                (17, 461., 490.),
                cx,
            )
            .await;
            let mut rows = 0;
            let mut each = |cx: &mut VisualTestContext,
                            stop: &str,
                            _: &[crate::focus_ring::PaintedOutline]| {
                let on_row = cx.update(|window, _| {
                    window.context_stack().iter().any(|context| {
                        context
                            .get(review::PART)
                            .is_some_and(|part| part.as_ref() == "pr-list")
                    })
                });
                if !on_row {
                    return;
                }
                rows += 1;
                let (body, drawn) = frame_lines(cx, &panel);
                assert_eq!(
                    cut_at_edges(body, &drawn),
                    Vec::<String>::new(),
                    "{stop}: the row's reveal leaves no line cut at the body's edges {body:?}"
                );
            };
            let mut walked = Vec::new();
            crate::focus_ring::sweep::walk_each(
                &app,
                cx,
                key,
                &format!("Pulls at 17 pt in 461 × 490 by {key}"),
                17,
                &mut walked,
                &mut each,
            );
            assert_eq!(rows, 1, "{key} reaches the PR row once");
            crate::focus_ring::sweep::assert_whole(walked);
        }
    }

    /// Tab onto the destination field, the part the panel reopens onto, from the banner's Move
    /// fixture head before it, and Shift+Tab onto it from Refresh PRs after it, draw the
    /// field's accent edge and the ring outside it whole on the field's frame, as every kit
    /// field draws them (`focus_ring::assert_input_ringed`), uncut by the body or any clip
    /// around it: on the PR list, at 13 and 18 pt in a 1,480 × 973 window and a 461 × 490 one,
    /// at scales 1.25 and 2.
    #[gpui::test]
    async fn tab_onto_the_destination_field_draws_its_ring_whole(cx: &mut TestAppContext) {
        let (_fixture, app, panel, pull, cx) = fixture_panel(cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        let field = cx.read(|cx| panel.read(cx).destination.read(cx).focus_handle(cx));
        let focused = |cx: &mut VisualTestContext| cx.update(|window, _| field.is_focused(window));
        let step = |cx: &mut VisualTestContext, key: &str| {
            cx.simulate_keystrokes(key);
            for _ in 0..2 {
                crate::width_walk::frame(cx);
            }
            // Moving the pointer without a press keeps the ring.
            cx.simulate_mouse_move(point(px(2.), px(2.)), None, Modifiers::none());
            crate::width_walk::frame(cx);
        };
        for scale in [1.25, 2.] {
            for (interface, width, height) in [
                (13, 1480., 973.),
                (18, 1480., 973.),
                (13, 461., 490.),
                (18, 461., 490.),
            ] {
                cx.update(|window, _| window.set_scale_factor(scale));
                show_surface(
                    (&app, &panel, &pull, &[]),
                    Surface::Pulls,
                    (interface, width, height),
                    cx,
                )
                .await;
                cx.update(|window, cx| window.focus(&field, cx));
                crate::width_walk::frame(cx);
                for (away, back) in [("shift-tab", "tab"), ("tab", "shift-tab")] {
                    let at = format!("{back} at {interface} pt in {width} × {height} at {scale}");
                    step(cx, away);
                    assert!(!focused(cx), "{at}: {away} leaves the field");
                    step(cx, back);
                    assert!(focused(cx), "{at}: {back} focuses the field");
                    let text = cx.read(|cx| panel.read(cx).destination.read(cx).input_bounds());
                    crate::focus_ring::assert_input_ringed(cx, text, &at);
                }
            }
        }
    }

    /// A row a list inside the body cuts at its edge is a line of the body as far as the list
    /// shows it (`review::keep_line`): at 18 pt in a 461 × 490 window, the changed-file list
    /// stacked above Changes shows its first row whole and the top of its second, which the
    /// body's lines hold from the row's top to the list's foot. Counted only where the list
    /// showed it whole, the second row reported nothing, and the body's reveals took the file
    /// name and status the list shows of it for room between lines: Tab onto the Review tab
    /// stopped the body's foot through that status line, as Tab onto Source's text at 13 pt
    /// stopped its top through the third row's file name, though no place the body could
    /// scroll to cleared every line there ([`needless_cut`]).
    #[gpui::test]
    async fn a_row_a_list_cuts_is_a_line_of_the_body_where_the_list_shows_it(
        cx: &mut TestAppContext,
    ) {
        let (_fixture, app, panel, pull, cx) = fixture_panel(cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        let captured = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let files = Client(UiTransport::Fixture { moved: false })
            .files(&captured, 1, &OperationControl::default())
            .unwrap()
            .items;
        show_surface(
            (&app, &panel, &pull, &files),
            Surface::Changes,
            (18, 461., 490.),
            cx,
        )
        .await;
        let slack = px(0.5);
        let list = cx.debug_bounds("github-file-rows").expect("the file list");
        let rows: Vec<Bounds<Pixels>> = indexed("github-file-row-", files.len())
            .into_iter()
            .filter_map(|row| cx.debug_bounds(row))
            .collect();
        let cut: Vec<&Bounds<Pixels>> = rows
            .iter()
            .filter(|row| row.top() < list.bottom() - slack && row.bottom() > list.bottom() + slack)
            .collect();
        let [row] = cut[..] else {
            panic!("the list {list:?} cuts one row at its foot: {rows:?}");
        };
        let lines = panel.read_with(cx, |panel, cx| panel.review.body_blocks.lines(cx));
        assert!(
            lines.iter().any(|(top, bottom)| {
                (*top - row.top()).abs() <= slack && (*bottom - list.bottom()).abs() <= slack
            }),
            "the row {row:?} the list {list:?} cuts is a line of the body to the list's foot: \
             {lines:?}"
        );
    }

    /// Fails with every finding of `swept` ([`sweep_reveals`]), each printed first.
    fn assert_swept(swept: Swept) {
        let findings: Vec<&String> = swept
            .cuts
            .iter()
            .chain(&swept.rings)
            .chain(&swept.unseen)
            .collect();
        for finding in &findings {
            eprintln!("{finding}");
        }
        assert!(swept.moves > 0, "the walks move the body");
        assert!(
            findings.is_empty(),
            "{} findings in {} stops, {} of them moving the body ({} onto a part too tall for it)",
            findings.len(),
            swept.stops,
            swept.moves,
            swept.tall
        );
    }

    /// The reveal sweep's walks at the stops where its run on 2026-11-05 found what FU20-GH6
    /// traced, which the whole sweep takes too long to repeat in every gate
    /// ([`the_reveal_sweep_finds_every_line_whole_and_every_ring_drawn`]): at 13 pt in a
    /// 461 × 490 window, Overview and Source; at 18 pt there, the connection pane, the PR list,
    /// Review, Changes, Source and Threads; at 13 pt in a 1,480 × 800 window, Threads. At every
    /// stop of each walk the focused control draws its ring whole, no reveal leaves a line cut
    /// where a place clears every line, and every line outside the lists is whole at some stop
    /// ([`assert_swept`]). Each surface at its size is a test of its own, walked with Tab and
    /// then with Shift+Tab, so a test runner takes them side by side: walked in one test, the
    /// nine took about 74 s in a debug build on the Linux development host and ran past
    /// nextest's 120 s limit on every hosted runner (run 42250019193). Threads in the wide
    /// window, whose two walks of 24 stops each take about 16 s there, is two tests, one for
    /// each key, and each walk shows every line outside the lists whole on its own.
    ///
    /// The connection pane at 18 pt in a 480 × 490 window, where Linux lays its help out in
    /// five lines as macOS does in 461 × 490, is a walk of its own: there no stop of its
    /// controls can show its "Local worktree:" line or its help's first line with every line
    /// whole, and hosted run 42333458175 found neither shown at any stop on macOS
    /// (`connection_at_18_pt`), as the walk did on Linux from 470 to 495 px wide. The pane is
    /// a stop of its own now, which shows both from its top. The PR list at 17 pt in a
    /// 461 × 490 window is one too, where FU27-B's probe found its row's reveals leaving a line
    /// cut ([`a_pr_rows_reveal_leaves_no_line_cut_at_17_pt`]).
    mod the_reveal_sweep_finds_nothing_where_it_found_reveals_to_trace {
        use super::*;

        /// Writes a test for each walk listed, a surface at an interface size in a window's
        /// width and height walked with the keys listed, that walks it through the reveal
        /// sweep ([`sweep_reveals`]) and fails with its findings ([`assert_swept`]).
        macro_rules! found_walks {
            ($($name:ident: $surface:ident at $size:expr, by $keys:expr;)*) => {$(
                #[gpui::test]
                async fn $name(cx: &mut TestAppContext) {
                    let walk: Walk<'_> = ($size, &[Surface::$surface]);
                    assert_swept(sweep_reveals(cx, &[walk], &$keys).await);
                }
            )*};
        }

        found_walks! {
            overview_at_13_pt: Overview at (13, 461., 490.), by BOTH_WAYS;
            source_at_13_pt: Source at (13, 461., 490.), by BOTH_WAYS;
            connection_at_18_pt: Connection at (18, 461., 490.), by BOTH_WAYS;
            connection_at_18_pt_in_a_480_px_window: Connection at (18, 480., 490.), by BOTH_WAYS;
            pulls_at_17_pt: Pulls at (17, 461., 490.), by BOTH_WAYS;
            pulls_at_18_pt: Pulls at (18, 461., 490.), by BOTH_WAYS;
            review_at_18_pt: Review at (18, 461., 490.), by BOTH_WAYS;
            changes_at_18_pt: Changes at (18, 461., 490.), by BOTH_WAYS;
            source_at_18_pt: Source at (18, 461., 490.), by BOTH_WAYS;
            threads_at_18_pt: Threads at (18, 461., 490.), by BOTH_WAYS;
            threads_at_13_pt_in_a_wide_window_by_tab: Threads at (13, 1480., 800.), by ["tab"];
            threads_at_13_pt_in_a_wide_window_by_shift_tab:
                Threads at (13, 1480., 800.), by ["shift-tab"];
        }
    }

    /// The reveal sweep over every surface of the panel ([`SURFACES`]) on the offline fixture,
    /// at 13 and 18 pt in a 461 × 490 window and a 1,480 × 800 one ([`REVEAL_SIZES`]), with
    /// Tab through every stop and then Shift+Tab ([`sweep_reveals`]): at every stop the focused
    /// control draws its ring whole, or its top edge where it cannot fit the body; no reveal
    /// leaves a line cut at either of the body's edges where a place clears every line with
    /// the revealed part whole; and every line outside the lists inside the body is whole at
    /// some stop. It prints every finding before it fails. It takes about 3 min in a debug
    /// build (180 s on the Linux development host on 2026-11-06), too long for every gate:
    /// the gate walks the stops where it found reveals to trace, a test for each walk
    /// ([`the_reveal_sweep_finds_nothing_where_it_found_reveals_to_trace`]), and this whole
    /// sweep stays opt-in.
    ///
    /// Run on Linux on 2026-11-05 (FU20-GH5), it found 133 in 872 stops. The 114 texts no stop
    /// showed were the conversation list's own rows, scrolled out of the list below its foot or
    /// above its top, which the sweep counted as the body's because only the rows crossing the
    /// list's viewport counted as the list's; the list's reveals are
    /// `conversation_reveals_leave_no_line_cut_at_the_lists_edges`'s, and the sweep now leaves
    /// whatever a list in the body holds to that list. No line outside the lists was never
    /// whole. The 19 left, traced in FU20-GH6 (2026-11-06), were three defects of the panel and
    /// three of the sweep's reading, each fixed; the sweep finds nothing since:
    /// - The PR list's first row at 18 pt in 461 × 490 (stops 2 and 3), its ring half a pixel
    ///   above the body's top: the reveal's stop clear of every line was allowed half a pixel
    ///   inside the part it shows (`review::clear_of_lines`).
    /// - Changes' patch at 18 pt in 461 × 490 (stops 4 and 10), its foot and ring below the
    ///   body's: it was revealed as the file's part, taller than the body, and is now a part of
    ///   its own, as Source's box is
    ///   (`reveals_draw_the_pr_rows_and_the_files_boxes_rings_whole`).
    /// - The Files section's third tab at 18 pt in 461 × 490 (Changes, Source and Threads,
    ///   stops 0, 14 and 22) and Source's text at 13 pt (stops 4 and 10): the changed-file
    ///   list's row its foot cut reported no line, so the reveals stopped through the status
    ///   line and the file name it showed of it; a row a list cuts is now the part it shows
    ///   (`review::keep_line`, `a_row_a_list_cuts_is_a_line_of_the_body_where_the_list_shows_it`).
    /// - Buttons write no accessibility node, so the sweep read no Button's frame and took its
    ///   label's run, 12 px at 13 pt, for its line: the run "no label or Button holds" at 13 pt
    ///   in Overview (stop 10) was Recover drafts' label, whose frame the body's foot cut, and
    ///   at 13 pt in 1,480 × 800, Threads' file pane (stop 4) had places through the frames of
    ///   the section tabs above it, and others through a line out of view (below). It reads
    ///   each control of one line as drawn now (`review::drawn_one_lines`).
    /// - Its places were clear of the lines its frame drew, and a frame draws nothing out of
    ///   the body's view: at 18 pt, the places for the review summary in Review (stop 15) and
    ///   for Source's text in Source (stops 4 and 10) cut the row of Buttons above them, out
    ///   of view where the reveal stopped, and Overview's at 13 pt cut the second line of
    ///   Drafts stay on this computer by half a pixel. It draws each place now, cut to the
    ///   reveal's hundredth of a pixel ([`verified_stop`]); none of them cleared every line.
    /// - It counted two runs of the History page's text beneath the dialog as lines of the
    ///   body at every stop, where their clips lay inside the body; it reads the dialog's text
    ///   above its backdrop alone now ([`backdrop`]).
    #[gpui::test]
    #[ignore = "Opt-in reveal sweep: run with --ignored; prints every finding"]
    async fn the_reveal_sweep_finds_every_line_whole_and_every_ring_drawn(cx: &mut TestAppContext) {
        let walks = REVEAL_SIZES.map(|size| (size, &SURFACES[..]));
        assert_swept(sweep_reveals(cx, &walks, &BOTH_WAYS).await);
    }

    /// A text of the panel works its lines out once for each change of what they follow from
    /// (`review::text_lines`), on a PR's Overview of the offline fixture at 13 pt in a
    /// 461 × 490 window: a frame that draws the panel unchanged reports every text's lines
    /// again with none worked out afresh, into the room the frames before took for them; a
    /// frame at another width works them out again, and reports the lines a frame working
    /// every text's out afresh reports. Every text read the window's text style for its lines
    /// in every frame, which folds the styles around it into a new one and allocates its font
    /// features, and the body took new room for its lines in every frame.
    #[gpui::test]
    async fn texts_work_their_lines_out_once_for_each_width(cx: &mut TestAppContext) {
        let (_fixture, app, panel, pull, cx) = fixture_panel(cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        show_overview(&panel, &pull, (13, 461., 490.), cx);
        let blocks = cx.read(|cx| panel.read(cx).review.body_blocks.clone());
        // The update's effects draw the window it dirtied, once, before it returns.
        let redraw = |cx: &mut VisualTestContext| {
            panel.update(cx, |_, cx| cx.notify());
            cx.run_until_parked();
        };
        redraw(cx);
        let (drawn, measured) = blocks.text_counts();
        let lines = cx.read(|cx| blocks.lines(cx));
        assert!(!lines.is_empty(), "the body collects its texts' lines");
        redraw(cx);
        let (drawn_again, measured_again) = blocks.text_counts();
        assert!(
            drawn_again > drawn,
            "the frame at rest draws the panel's texts again"
        );
        assert_eq!(
            measured_again, measured,
            "a frame at rest works out no text's lines again"
        );
        assert_eq!(
            cx.read(|cx| blocks.lines(cx)),
            lines,
            "and reports the lines the frame before did"
        );
        assert!(
            blocks.keeps_its_room(),
            "the frame at rest records its lines in the room the frame before took"
        );

        cx.simulate_resize(size(px(441.), px(490.)));
        for _ in 0..3 {
            crate::width_walk::frame(cx);
        }
        let (_, resized) = blocks.text_counts();
        assert!(
            resized > measured,
            "a frame at another width works the texts' lines out again"
        );
        let kept = cx.read(|cx| blocks.lines(cx));
        blocks.forget_texts();
        redraw(cx);
        assert_eq!(
            cx.read(|cx| blocks.lines(cx)),
            kept,
            "the lines a frame takes from the one before are those it works out afresh"
        );
    }

    /// Tab through a PR's Overview on the offline fixture, at 13 and 18 pt in a 461 × 490
    /// window and in a 1,480 × 800 one: where a stop scrolls the body or the conversation list,
    /// the scrolled box's top edge lies at the start of a block, cutting no line, as long as
    /// the box has content left below to show; and wrapping from the dialog's last stop to its
    /// first, the banner's, returns the body to its top. A conversation's reveal once cut its
    /// author line at the list's top, and the wrap left the body 6.5 px (13 pt) and 9 px (18
    /// pt) down, the banner's top cut. Every stop draws its ring whole (`focus_ring::sweep`),
    /// but for the conversations pane, which is taller than the body in a 461 × 490 window and
    /// shows its top edge there, its bottom below the fold.
    #[gpui::test]
    async fn tab_reveals_land_on_whole_lines_and_wrap_to_the_body_top(cx: &mut TestAppContext) {
        use crate::focus_ring::sweep;
        let (_fixture, app, panel, pull, cx) = fixture_panel(cx).await;
        let (body_scroll, list_scroll) = cx.read(|cx| {
            let panel = panel.read(cx);
            (
                panel.review.body_scroll.clone(),
                panel.conversations.list_scroll.clone(),
            )
        });
        let mut rings = Vec::new();
        for sizes in REVEAL_SIZES {
            let (interface, width, height) = sizes;
            let at = format!("{interface} pt in {width} × {height}");
            show_overview(&panel, &pull, sizes, cx);
            // The case the narrow window is here for, which the rings' known failures below
            // count on: the conversations pane, with its ring's room above and below it,
            // taller than the body, so it shows its top there and its foot below the fold.
            // At 13 pt it is taller by about 9 px, and a layout change that let it fit would
            // otherwise fail only as known failures that no longer happen.
            if width < 1000. {
                let body = cx.debug_bounds("github-panel").expect("the body");
                let pane = cx.debug_bounds("github-thread-section").expect("the pane");
                let room = cx.update(|_, cx| appearance::button_ring_room(cx));
                assert!(
                    pane.size.height + room * 2. > body.size.height,
                    "{at}: the pane {pane:?} with its ring's room is taller than the body {body:?}"
                );
            }
            let mut offsets = (body_scroll.offset().y, list_scroll.offset().y);
            let mut wrapped = false;
            let mut list_moves = 0;
            for stop in 0..60 {
                cx.simulate_keystrokes("tab");
                for _ in 0..3 {
                    crate::width_walk::frame(cx);
                }
                let part = cx.update(|window, _| {
                    window
                        .context_stack()
                        .iter()
                        .rev()
                        .find_map(|context| context.get(review::PART).cloned())
                });
                let now = (body_scroll.offset().y, list_scroll.offset().y);
                let ends = (-body_scroll.max_offset().y, -list_scroll.max_offset().y);
                let here = format!("{at}, stop {stop} in {part:?}");
                if now.0 != offsets.0 && now.0 > ends.0 + px(0.5) {
                    let body = cx.debug_bounds("github-panel").expect("the body");
                    let cut = lines_cut_at_top(cx, body);
                    assert!(
                        cut.is_empty(),
                        "{here}: the body's top edge {body:?} cuts {cut:?}"
                    );
                }
                if now.1 != offsets.1 {
                    list_moves += 1;
                    let list = cx.debug_bounds("github-threads").expect("the list");
                    let cut = lines_cut_at_top(cx, list);
                    if now.1 > ends.1 + px(0.5) {
                        assert!(
                            cut.is_empty(),
                            "{here}: the list's top edge {list:?} cuts {cut:?}"
                        );
                    }
                }
                offsets = now;
                if part.as_ref().is_some_and(|part| part.as_ref() == "banner") {
                    assert_eq!(
                        now.0,
                        px(0.),
                        "{here}: the wrap returns the body to its top"
                    );
                    wrapped = true;
                    break;
                }
            }
            assert!(wrapped, "{at}: Tab wraps to the banner");
            assert!(list_moves > 0, "{at}: Tab scrolls the conversation list");
            // Each stop's ring, Tab round from the pane.
            show_overview(&panel, &pull, sizes, cx);
            let mut failures = Vec::new();
            sweep::walk(&app, cx, &at, interface, &mut failures);
            rings.extend(failures);
        }
        sweep::assert_whole_except(
            rings,
            &[
                (
                    "13 pt in 461 × 490 at 13 pt, stop 21",
                    "a clip cuts the right, bottom, left",
                ),
                (
                    "18 pt in 461 × 490 at 18 pt, stop 21",
                    "a clip cuts the right, bottom, left",
                ),
            ],
        );
    }

    /// Closing the panel with Escape after Tab has scrolled a PR's Overview, and opening it
    /// again, restores focus to the conversations pane, which draws its keyboard ring on its
    /// edge with its top edge drawn inside the body's clip, the room a ring takes clear of it:
    /// in a 461 × 490 window, at 13 and 18 pt, the pane is taller than the body and once showed
    /// its bottom with its top above the clip. In a 1,480 × 800 window the pane fits, and its
    /// ring is whole.
    #[gpui::test]
    async fn a_reopened_prs_conversations_pane_draws_its_rings_top_edge(cx: &mut TestAppContext) {
        let (_fixture, app, panel, pull, cx) = fixture_panel(cx).await;
        for sizes in REVEAL_SIZES {
            let (interface, width, height) = sizes;
            let at = format!("{interface} pt in {width} × {height}");
            show_overview(&panel, &pull, sizes, cx);
            // Into the conversation list, scrolling the body down to it.
            cx.simulate_keystrokes("tab tab");
            crate::width_walk::frame(cx);
            escape(cx);
            assert!(
                !cx.update(|window, cx| window.has_active_dialog(cx)),
                "{at}: Escape closes the panel"
            );
            cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
            for _ in 0..4 {
                crate::width_walk::frame(cx);
            }
            let section = cx.read(|cx| panel.read(cx).review.section_focus.clone());
            assert!(
                cx.update(|window, _| section.is_focused(window)),
                "{at}: the reopened panel restores focus to the conversations pane"
            );
            let (body, pane) = (
                cx.debug_bounds("github-panel").expect("the body"),
                cx.debug_bounds("github-thread-section").expect("the pane"),
            );
            let accent: Hsla = cx.read(|cx| rgb(palette(cx).accent).into());
            let rings: Vec<_> = crate::focus_ring::painted_outlines(cx, accent)
                .into_iter()
                .filter(|ring| near(ring.bounds, pane))
                .collect();
            let [ring] = rings[..] else {
                panic!("{at}: one ring on the pane's edge {pane:?}: {rings:?}");
            };
            let room = cx.update(|_, cx| appearance::button_ring_room(cx));
            assert!(
                ring.sides[0] && pane.top() >= body.top() + room - px(0.5),
                "{at}: the ring's top edge lies inside the body {body:?}, its room clear: {ring:?}"
            );
            // The case the narrow window is here for: a pane taller than the body, which a
            // taller body would otherwise turn into a pane that fits without failing.
            if width < 1000. {
                assert!(
                    pane.size.height + room * 2. > body.size.height,
                    "{at}: the pane {pane:?} is taller than the body {body:?}"
                );
            }
            if pane.size.height + room * 2. <= body.size.height {
                assert!(
                    ring.whole,
                    "{at}: the pane fits, and its ring is whole: {ring:?}"
                );
            }
            if width > 1000. {
                assert!(
                    ring.whole,
                    "{at}: the wide window fits the pane whole: {ring:?}"
                );
            }
        }
    }

    /// The panel closed with its body scrolled to its end reopens onto its destination field
    /// with the body back at its top, at 13 and 18 pt in a 461 × 490 window: the field whole
    /// with its ring and the banner above it in view, rather than at the offset it closed at:
    /// at 13 pt it reopened 93 px down, the field just inside the body's top edge and the
    /// banner above it hidden, since the field alone was whole and nothing revealed more.
    #[gpui::test]
    async fn reopening_after_scrolling_returns_the_body_to_its_top(cx: &mut TestAppContext) {
        use crate::tags::tests::draw;
        let (_fixture, app, panel, cx) = notice_panel(cx);
        let (scroll, field) = cx.read(|cx| {
            let panel = panel.read(cx);
            (
                panel.review.body_scroll.clone(),
                panel.destination.read(cx).focus_handle(cx),
            )
        });
        let room = cx.update(|_, cx| appearance::button_ring_room(cx));
        let mut scrolled = 0;
        for interface in [13, 18] {
            let at = format!("{interface} pt");
            open_notice_panel(&app, &panel, (interface, 461., 490.), cx).await;
            if scroll.max_offset().y <= px(0.) {
                escape(cx);
                continue;
            }
            scrolled += 1;
            scroll.set_offset(point(px(0.), -scroll.max_offset().y));
            draw(cx);
            draw(cx);
            escape(cx);
            cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
            settle_panel(&app, cx).await;
            for _ in 0..3 {
                draw(cx);
            }
            assert!(
                cx.update(|window, _| field.is_focused(window)),
                "{at}: the panel reopens onto its destination field"
            );
            assert_eq!(
                scroll.offset().y,
                px(0.),
                "{at}: the body is back at its top"
            );
            let body = cx.debug_bounds("github-panel").expect("the body");
            let destination = cx
                .debug_bounds("github-destination")
                .expect("the destination field");
            let banner = cx.debug_bounds("github-fixture-mode").expect("the banner");
            assert!(
                destination.top() - room >= body.top()
                    && destination.bottom() + room <= body.bottom()
                    && banner.top() >= body.top(),
                "{at}: the field {destination:?} with its ring and the banner {banner:?} lie in \
                 view in {body:?}"
            );
            escape(cx);
        }
        assert!(scrolled > 0, "some size scrolls the body");
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Reopening the panel onto its destination field keeps the body at its top wherever the
    /// field is whole there with its ring, as at 13 and 18 pt in a 461 × 490 window, so the
    /// banner and its Move fixture head stay in view, whether a key or the pointer came last.
    /// At 18 pt it scrolled the body down, by 129.5 px the field's row to its top, and the
    /// banner out of view: the field kept a line's room clear of the body's edges rather than
    /// its ring's, which alone scrolled 112 px, and after a key its row, taller than the body,
    /// revealed itself rather than the field. In a 461 × 360 window, where the field is not
    /// whole at the body's top, reopening scrolls the body until it is. A PR's Review reopens
    /// onto its summary, a field too: where the body shows it whole with its ring, at 13 pt in
    /// a 461 × 490 window and 18 pt in a 1,480 × 800 one, the body stays where it was, where a
    /// line's room clear of its edges scrolled it on.
    #[gpui::test]
    async fn reopening_onto_a_whole_field_keeps_the_body_at_its_top(cx: &mut TestAppContext) {
        use crate::tags::tests::draw;
        let (_fixture, app, panel, cx) = notice_panel(cx);
        let (scroll, field) = cx.read(|cx| {
            let panel = panel.read(cx);
            (
                panel.review.body_scroll.clone(),
                panel.destination.read(cx).focus_handle(cx),
            )
        });
        let room = cx.update(|_, cx| appearance::button_ring_room(cx));
        for (interface, width, height) in [(13, 461., 490.), (18, 461., 490.), (18, 461., 360.)] {
            for pointer in [false, true] {
                let at = format!(
                    "{interface} pt in {width} × {height}, reopened after the {}",
                    if pointer { "pointer" } else { "keyboard" }
                );
                cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
                cx.simulate_resize(size(px(width), px(height)));
                draw(cx);
                cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
                settle_panel(&app, cx).await;
                escape(cx);
                assert!(
                    !cx.update(|window, cx| window.has_active_dialog(cx)),
                    "{at}: Escape closes the panel"
                );
                // The body at its top, as a panel opens that nothing has scrolled.
                scroll.set_offset(point(px(0.), px(0.)));
                if pointer {
                    cx.simulate_mouse_move(point(px(4.), px(4.)), None, Modifiers::default());
                }
                cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
                settle_panel(&app, cx).await;
                for _ in 0..3 {
                    crate::width_walk::frame(cx);
                }
                assert!(
                    cx.update(|window, _| field.is_focused(window)),
                    "{at}: the panel reopens onto the destination field"
                );
                let offset = scroll.offset().y;
                let body = cx.debug_bounds("github-panel").expect("the body");
                let shown = cx.debug_bounds("github-destination").expect("the field");
                assert!(
                    shown.top() - room >= body.top() - px(0.5)
                        && shown.bottom() + room <= body.bottom() + px(0.5),
                    "{at}: the field {shown:?} is whole with its ring in the body {body:?}"
                );
                // Where the field lies with the body at its top.
                let (top, bottom) = (shown.top() - offset, shown.bottom() - offset);
                let whole_at_top =
                    top - room >= body.top() - px(0.5) && bottom + room <= body.bottom() + px(0.5);
                assert_eq!(
                    whole_at_top,
                    height > 480.,
                    "{at}: the field is whole at the body's top in the taller window only"
                );
                if whole_at_top {
                    assert_eq!(offset, px(0.), "{at}: the body stays at its top");
                    let banner = cx
                        .debug_bounds("github-fixture-move")
                        .expect("Move fixture head");
                    assert!(
                        banner.top() >= body.top() && banner.bottom() <= body.bottom(),
                        "{at}: Move fixture head {banner:?} stays in view in {body:?}"
                    );
                } else {
                    assert!(offset < px(0.), "{at}: the body scrolls to the field");
                }
                escape(cx);
            }
        }

        // A PR's Review, reopened onto its summary where the body shows it whole with its
        // ring and less than a line's room below it.
        let summary = cx.read(|cx| panel.read(cx).body.read(cx).focus_handle(cx));
        cx.update(|_, cx| {
            panel.update(cx, |panel, _| {
                panel.pull = Some(github::review::fixture_pull(false));
                panel.review.section = Section::Review;
                panel.review.show_list = false;
            })
        });
        // Opened and closed once, so the draft status the destination's close left ("Draft
        // not saved: choose a pull request…", three lines at 13 pt in 461 × 490) gives way to
        // the PR's own, and nothing above the summary moves across the reopens below: the
        // status shrank by 39 px across the first one and took the summary out of the place
        // that leaves it less than a line's room.
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
        settle_panel(&app, cx).await;
        escape(cx);
        for (interface, width, height) in [(13, 461., 490.), (18, 1480., 800.)] {
            for pointer in [false, true] {
                let at = format!(
                    "Review at {interface} pt in {width} × {height}, reopened after the {}",
                    if pointer { "pointer" } else { "keyboard" }
                );
                cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
                cx.simulate_resize(size(px(width), px(height)));
                draw(cx);
                cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
                settle_panel(&app, cx).await;
                for _ in 0..3 {
                    crate::width_walk::frame(cx);
                }
                let body = cx.debug_bounds("github-panel").expect("the body");
                let shown = cx
                    .debug_bounds("github-review-summary")
                    .expect("the summary");
                let line = cx.update(|window, _| window.rem_size()) + px(2.);
                let placed = (scroll.offset().y + (body.bottom() - room - px(1.) - shown.bottom()))
                    .clamp(-scroll.max_offset().y, px(0.));
                scroll.set_offset(point(px(0.), placed));
                draw(cx);
                draw(cx);
                let shown = cx
                    .debug_bounds("github-review-summary")
                    .expect("the summary");
                assert!(
                    shown.top() - room >= body.top()
                        && shown.bottom() + room <= body.bottom()
                        && shown.bottom() + line > body.bottom(),
                    "{at}: the summary {shown:?} is whole with its ring in the body {body:?}, \
                     with less than a line's room below it"
                );
                escape(cx);
                if pointer {
                    cx.simulate_mouse_move(point(px(4.), px(4.)), None, Modifiers::default());
                }
                cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
                settle_panel(&app, cx).await;
                for _ in 0..3 {
                    crate::width_walk::frame(cx);
                }
                assert!(
                    cx.update(|window, _| summary.is_focused(window)),
                    "{at}: the panel reopens onto the summary"
                );
                assert_eq!(
                    scroll.offset().y,
                    placed,
                    "{at}: the body stays where it was"
                );
                assert_eq!(
                    cx.debug_bounds("github-review-summary"),
                    Some(shown),
                    "{at}: the summary stays where it was placed"
                );
                escape(cx);
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Switching a PR's section moves nothing already drawn: the body keeps its scrollbar's
    /// gutter whether or not the section overflows, so the content's right edge, where the
    /// Buttons and the draft status at the panel's right end, and the tabs stay put. At 13 pt
    /// in a 1,480 × 980 window Overview overflows and Files fits, and the right edge moved
    /// 19 px between them, the gutter's track and the ring's room.
    #[gpui::test]
    async fn switching_a_prs_section_moves_nothing(cx: &mut TestAppContext) {
        let (_fixture, app, panel, pull, cx) = fixture_panel(cx).await;
        let captured = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let files = Client(UiTransport::Fixture { moved: false })
            .files(&captured, 1, &OperationControl::default())
            .unwrap()
            .items;
        let scroll = cx.read(|cx| panel.read(cx).review.body_scroll.clone());
        // Whether some window showed one section overflowing and another fitting.
        let mut mixed = false;
        for (interface, width, height) in [
            (13, 1480., 980.),
            (18, 1480., 980.),
            (13, 1000., 680.),
            (18, 1000., 680.),
            (13, 461., 490.),
            (18, 461., 490.),
        ] {
            let at = format!("{interface} pt in {width} × {height}");
            crate::focus_ring::sweep::interface_size(cx, interface);
            cx.simulate_resize(size(px(width), px(height)));
            let mut drawn = Vec::new();
            for section in [
                Section::Overview,
                Section::Files,
                Section::Review,
                Section::Overview,
            ] {
                cx.update(|window, cx| {
                    panel.update(cx, |panel, cx| {
                        panel.pull = Some(pull.clone());
                        panel.review.files = files.clone();
                        panel.review.show_list = false;
                        panel.change_section(section, window, cx);
                    })
                });
                scroll.set_offset(point(px(0.), px(0.)));
                settle_panel(&app, cx).await;
                for _ in 0..2 {
                    crate::width_walk::frame(cx);
                }
                let content = cx
                    .debug_bounds("github-panel-content")
                    .expect("the content");
                let tab = cx
                    .debug_bounds("github-section-0")
                    .expect("the Overview tab");
                drawn.push((
                    section,
                    content.right(),
                    tab,
                    scroll.max_offset().y > px(0.),
                ));
            }
            let overflows = drawn
                .iter()
                .filter(|(_, _, _, overflows)| *overflows)
                .count();
            mixed |= overflows > 0 && overflows < drawn.len();
            let (_, right, tab, _) = drawn[0];
            assert!(
                drawn
                    .iter()
                    .all(|(_, other, place, _)| *other == right && *place == tab),
                "{at}: switching sections moved the content's right edge or the tabs: {drawn:?}"
            );
        }
        assert!(
            mixed,
            "some window shows one section overflowing the body and another fitting it"
        );
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Opening a PR from the list, by the keyboard, at 13 and 18 pt in a 461 × 490 window shows
    /// it from its title: the title whole in the body with Overview, the tab it opens to and its
    /// first stop, whole below it, and at 13 pt its captured identities too, where the PR once
    /// opened showing only its title, the tabs below the fold (Overview's top at 464 px against
    /// a body ending at 355.5 px at 13 pt). At 18 pt the tabs wrap, and Review's row lies below
    /// the fold. In a 1,480 × 800
    /// window, which shows all of that as it is, the body stays at its top, the controls and
    /// notices above the PR in view.
    #[gpui::test]
    async fn opening_a_pr_shows_it_from_its_title(cx: &mut TestAppContext) {
        let (_fixture, app, panel, pull, cx) = fixture_panel(cx).await;
        let body_scroll = cx.read(|cx| panel.read(cx).review.body_scroll.clone());
        for (interface, width, height) in REVEAL_SIZES {
            let at = format!("{interface} pt in {width} × {height}");
            crate::focus_ring::sweep::interface_size(cx, interface);
            cx.simulate_resize(size(px(width), px(height)));
            cx.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    panel.pull = None;
                    panel.review.show_list = true;
                    panel.review.body_scroll.set_offset(point(px(0.), px(0.)));
                    panel.focus_visible_section(window, cx);
                    cx.notify();
                })
            });
            crate::width_walk::frame(cx);
            // The keyboard drives, as Return on a PR row does: focus reveals.
            cx.simulate_keystrokes("tab");
            crate::width_walk::frame(cx);
            body_scroll.set_offset(point(px(0.), px(0.)));
            cx.update(|window, cx| {
                panel.update(cx, |panel, cx| panel.inspect(pull.clone(), window, cx))
            });
            settle_panel(&app, cx).await;
            for _ in 0..3 {
                crate::width_walk::frame(cx);
            }
            assert!(
                cx.read(|cx| panel.read(cx).pull.is_some() && !panel.read(cx).review.show_list),
                "{at}: the PR opened"
            );
            let body = cx.debug_bounds("github-panel").expect("the body");
            let inside = |bounds: Bounds<Pixels>| {
                bounds.top() >= body.top() - px(0.5) && bounds.bottom() <= body.bottom() + px(0.5)
            };
            let title = cx.debug_bounds("github-pull-title").expect("the title");
            assert!(
                inside(title),
                "{at}: the title {title:?} lies whole in the body {body:?}"
            );
            let overview = cx
                .debug_bounds("github-section-0")
                .expect("the Overview tab");
            assert!(
                inside(overview),
                "{at}: Overview {overview:?}, the PR's first stop, lies whole in the body \
                 {body:?}, below the title {title:?} and the branches {:?}",
                cx.debug_bounds("github-branch-context")
            );
            if interface == 13 {
                // And the summary of what the PR compares, below the tabs.
                let identities = cx
                    .debug_bounds("github-captured-identities")
                    .expect("the captured identities");
                assert!(
                    inside(identities),
                    "{at}: the captured identities {identities:?} lie whole in the body {body:?}"
                );
            }
            if width > 1000. {
                assert_eq!(
                    body_scroll.offset().y,
                    px(0.),
                    "{at}: the wide window shows the PR without scrolling"
                );
            }
        }
    }

    /// Escape, the frame after the dialog closes, and the app's repair of
    /// modal focus 300 ms later.
    fn escape(cx: &mut VisualTestContext) {
        cx.simulate_keystrokes("escape");
        crate::tags::tests::draw(cx);
        crate::copy_feedback::tests::wait(cx, Duration::from_millis(400));
        crate::tags::tests::draw(cx);
    }

    /// The last frame's quads with their bounds in logical pixels.
    fn quads(cx: &mut VisualTestContext) -> Vec<(Bounds<Pixels>, gpui_kit::Quad)> {
        cx.update(|window, _| {
            let scale = window.scale_factor();
            window
                .painted_quads()
                .into_iter()
                .map(|quad| {
                    let b = quad.bounds;
                    let bounds = Bounds::from_corners(
                        point(px(b.left().as_f32() / scale), px(b.top().as_f32() / scale)),
                        point(
                            px(b.right().as_f32() / scale),
                            px(b.bottom().as_f32() / scale),
                        ),
                    );
                    (bounds, quad)
                })
                .collect()
        })
    }

    /// The open panel's dialog frame as the last frame painted it, the smallest edged quad
    /// around the body and the footer, with its cap's foot: [`DIALOG_MARGIN`] above the
    /// status bar's top edge, where [`appearance::dialog_max_height`] ends a dialog.
    ///
    /// [`DIALOG_MARGIN`]: appearance::DIALOG_MARGIN
    pub(super) fn dialog_frame(cx: &mut VisualTestContext) -> (Bounds<Pixels>, Pixels) {
        let body = cx.debug_bounds("github-panel").expect("the body");
        let close = cx.debug_bounds("github-close").expect("Back to repository");
        let foot = cx.update(|window, _| {
            crate::theme_editor::status_bar_top(window) - appearance::DIALOG_MARGIN
        });
        let dialog = quads(cx)
            .into_iter()
            .filter(|(bounds, quad)| {
                let widths = quad.border_widths;
                [widths.top, widths.right, widths.bottom, widths.left]
                    .iter()
                    .all(|width| width.as_f32() > 0.)
                    && bounds.top() < body.top()
                    && bounds.bottom() > close.bottom()
                    && bounds.left() < body.left()
                    && bounds.right() > body.right()
            })
            .map(|(bounds, _)| bounds)
            .min_by(|a, b| a.size.height.partial_cmp(&b.size.height).unwrap())
            .expect("the dialog's frame");
        (dialog, foot)
    }

    fn near(a: Bounds<Pixels>, b: Bounds<Pixels>) -> bool {
        [
            (a.left(), b.left()),
            (a.top(), b.top()),
            (a.right(), b.right()),
            (a.bottom(), b.bottom()),
        ]
        .into_iter()
        .all(|(a, b)| (a - b).abs() < px(0.01))
    }

    /// The window painted the installed Button focus ring around `selector`.
    fn ringed(cx: &mut VisualTestContext, selector: &'static str) -> bool {
        let control = cx.debug_bounds(selector).expect(selector);
        let (ring, color, scale) = cx.update(|window, cx| {
            let theme = gpui_kit::component::Theme::global(cx);
            let ring = theme.button_focus_ring;
            (ring, theme.ring.alpha(ring.opacity), window.scale_factor())
        });
        assert_eq!(ring, appearance::BUTTON_FOCUS_RING);
        let around = control.dilate(ring.gap + ring.width);
        quads(cx).into_iter().any(|(bounds, quad)| {
            quad.background.is_transparent()
                && quad.border_color == color
                && quad.border_widths.top.as_f32() == ring.width.as_f32() * scale
                && near(bounds, around)
        })
    }

    /// `selector` painted its 1 px keyboard-focus outline in the accent.
    fn outlined(cx: &mut VisualTestContext, selector: &'static str) -> bool {
        let pane = cx.debug_bounds(selector).expect(selector);
        let (accent, scale) =
            cx.update(|window, cx| (Hsla::from(rgb(palette(cx).accent)), window.scale_factor()));
        quads(cx).into_iter().any(|(bounds, quad)| {
            quad.border_color == accent
                && quad.border_widths.top.as_f32() == scale
                && near(bounds, pane)
        })
    }

    /// The panel opened from the active repository's update, as command
    /// palette activation opens it, with its local reads drained.
    async fn open_panel(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            // This is the same parent-entity update boundary as command-palette
            // activation. The previous constructor synchronously reborrowed it.
            app.update(cx, |app, cx| app.open_github(window, cx));
            assert!(window.has_active_dialog(cx));
        });
        settle_panel(app, cx).await;
    }

    /// The open panel with its local reads drained.
    async fn settle_panel(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        let (operations_done, preferences_done) = app.read_with(cx, |app, _| {
            (
                app.operations.submit_read(|| Ok(())),
                app.preferences_writer.submit_read(|| Ok(())),
            )
        });
        operations_done.await.unwrap().unwrap();
        preferences_done.await.unwrap().unwrap();
        cx.executor().run_until_parked();
    }

    /// Asserts that the [`phrases`] label `selector`, drawing `text` at `size`,
    /// breaks its lines only between phrases, and returns how many it drew.
    /// Each line the last frame drew there is as wide as a run of whole
    /// phrases shaped in one line, give or take a separator's trailing space.
    /// The window's root marks every text run with `width_walk::text_mark`.
    pub(super) fn assert_phrases_whole(
        cx: &mut VisualTestContext,
        selector: &'static str,
        text: &str,
        size: Pixels,
        at: &str,
    ) -> usize {
        let bounds = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{at}: {selector}"));
        let mark = gpui_kit::Background::from(crate::width_walk::text_mark());
        let (scale, quads) = cx.update(|window, _| (window.scale_factor(), window.painted_quads()));
        let logical = |value: gpui_kit::ScaledPixels| px(value.0 / scale);
        // Each line's top, left and right.
        let mut lines: Vec<(Pixels, Pixels, Pixels)> = Vec::new();
        for quad in quads.iter().filter(|quad| quad.background == mark) {
            let (left, top) = (logical(quad.bounds.left()), logical(quad.bounds.top()));
            let (right, bottom) = (logical(quad.bounds.right()), logical(quad.bounds.bottom()));
            if left < bounds.left() - px(0.5)
                || right > bounds.right() + px(0.5)
                || top < bounds.top() - px(0.5)
                || bottom > bounds.bottom() + px(0.5)
            {
                continue;
            }
            match lines.iter_mut().find(|line| (line.0 - top).abs() < px(0.5)) {
                Some(line) => {
                    line.1 = line.1.min(left);
                    line.2 = line.2.max(right);
                }
                None => lines.push((top, left, right)),
            }
        }
        assert!(!lines.is_empty(), "{at}: {selector} drew no text");
        let parts: Vec<&str> = text.split(" · ").collect();
        let runs: Vec<(String, Pixels)> = cx.update(|window, _| {
            let mut runs = Vec::new();
            for start in 0..parts.len() {
                for end in start..parts.len() {
                    let mut run = parts[start..=end].join(" · ");
                    if end + 1 < parts.len() {
                        run.push_str(" ·");
                    }
                    for run in [run.clone(), format!("{run} ")] {
                        let width =
                            crate::views::shaped_width(&run, size, FontWeight::NORMAL, window);
                        runs.push((run, width));
                    }
                }
            }
            runs
        });
        // Layout puts each phrase's edges on device pixels, so a line of several phrases can
        // be a pixel or two wider than its run shaped whole; runs that differ by a word differ
        // by far more than this.
        let slack = size / 4.;
        for &(top, left, right) in &lines {
            let width = right - left;
            assert!(
                runs.iter().any(|(_, run)| (width - *run).abs() < slack),
                "{at}: {selector}'s line at {top:?} is {width:?} wide, which no run of whole \
                 phrases is: it breaks inside a phrase. Lines {lines:?}, runs {runs:?}"
            );
        }
        lines.len()
    }

    /// A window on an empty repository, on the Repository page. Keep the
    /// returned directory until the test ends.
    ///
    /// The repository is made in /tmp, so its path, which the connection
    /// pane's "Local worktree:" line shows above the help, is short on every
    /// host (`/tmp/.tmp…` on Linux, `/private/tmp/.tmp…` on macOS), as
    /// `worktrees::ring_room_tests::short_fixture`'s is, and the line shows it
    /// whole. The line keeps to one line whatever the path's length, eliding
    /// it in its middle ([`long_path_window`]).
    fn repository_window(
        cx: &mut TestAppContext,
    ) -> (tempfile::TempDir, Entity<GitTurtle>, &mut VisualTestContext) {
        let fixture = tempfile::tempdir_in("/tmp").unwrap();
        let path = fixture.path().to_path_buf();
        repository_window_at(cx, fixture, &path)
    }

    /// A [`repository_window`] on a repository whose path is about 120 characters long, made
    /// in a directory of its own under /tmp, outside any work tree, its last component
    /// "compact": `/tmp/.tmp…/gitturtle-github-connection-pane/a-local-worktree-…/compact`,
    /// 118 characters on Linux, whatever TMPDIR's length.
    fn long_path_window(
        cx: &mut TestAppContext,
    ) -> (tempfile::TempDir, Entity<GitTurtle>, &mut VisualTestContext) {
        let fixture = tempfile::tempdir_in("/tmp").unwrap();
        let path = fixture
            .path()
            .join("gitturtle-github-connection-pane")
            .join("a-local-worktree-path-long-enough-to-wrap-at-every-text-size")
            .join("compact");
        std::fs::create_dir_all(&path).unwrap();
        repository_window_at(cx, fixture, &path)
    }

    /// A [`repository_window`] on an empty repository made at `path`, inside `fixture`.
    fn repository_window_at<'a>(
        cx: &'a mut TestAppContext,
        fixture: tempfile::TempDir,
        path: &std::path::Path,
    ) -> (
        tempfile::TempDir,
        Entity<GitTurtle>,
        &'a mut VisualTestContext,
    ) {
        // This full app integration test intentionally performs local reads on
        // the real serial executors. GPUI's supported I/O mode accepts their
        // cross-thread wakes; the barriers in `open_panel` drain them before
        // teardown.
        cx.executor().allow_parking();
        let result = std::process::Command::new("git")
            .args(["-c", "init.templateDir=", "init", "--initial-branch=main"])
            .arg(path)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(result.status.success());
        crate::views::tests::fixture_identity(path);
        let repo = GitRepository::open(path).unwrap();
        let view: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let captured = view.clone();
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
                app.repository = Some(repo);
                app
            });
            *captured.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let app = view.borrow().as_ref().unwrap().clone();
        (fixture, app, cx)
    }
}
