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
use gpui_kit::base::ElementExt as _;
use gpui_kit::{
    component::{WindowExt, dialog::DialogFooter},
    prelude::FluentBuilder,
};
use std::time::Duration;
mod conversations;
#[cfg(test)]
mod phrase_bench;
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
/// breaks between two phrases, and inside a phrase only where that phrase
/// alone is wider than the line, there between two of its words, never inside
/// a key name or before its separator ([`crate::PhraseLine`]). Each phrase
/// draws as `{id}-{index}`, the separator ending the phrase before it; on one
/// line they read as the text does, and the accessible name is the whole
/// text. Like a [`label`]'s, its lines are never left cut at the body's edge.
///
/// A phrase wider than the line wraps at the line's width, the width the
/// label is drawn in: as a wrapping item it may shrink below its width on one
/// line. Changes' range hint, in a 461 × 490 window at 18 pt, laid its second
/// phrase out on one line 649 px wide in a 344 px label and drew it under the
/// body's scrollbar, cut at the body's edge (QA-106d).
fn phrases(
    id: &'static str,
    text: impl Into<SharedString>,
) -> native_accessibility::StaticText<Stateful<Div>> {
    let text = text.into();
    let lines = ElementId::from(id);
    native_accessibility::static_text(
        div()
            .on_children_prepainted(move |children, window, cx| {
                review::text_lines(&lines, children, window, cx)
            })
            .id(id)
            .debug_selector(move || id.into())
            .child(crate::PhraseLine::new(id, &text)),
        text,
    )
    .text_size(crate::appearance::ui_text(12.))
}
/// `button` with `name` as its accessible name where its label alone would not say what it
/// acts on. In tests the name is kept under `selector` ([`NAMED_FOR_READING`]): the test
/// platform builds no accessibility tree, and a kit Button's node is not reachable.
fn named_for_reading(button: Button, selector: &'static str, name: String) -> Button {
    #[cfg(test)]
    NAMED_FOR_READING.with(|named| named.borrow_mut().insert(selector, name.clone()));
    let _ = selector;
    button.accessibility_label(name)
}
#[cfg(test)]
thread_local! {
    /// The accessible names [`named_for_reading`] last gave, by selector.
    static NAMED_FOR_READING: std::cell::RefCell<std::collections::BTreeMap<&'static str, String>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
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
    pane.py(PANE_PADDING_Y).px(text_inset() - px(1.))
}
/// The padding above and below a bordered pane's content ([`bordered_pane`]), which a reveal's
/// mark over that content stands inside (`review::RevealMark`).
const PANE_PADDING_Y: gpui::Rems = rems(0.75);
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
/// `element`, recording the layout ID it is laid out under in `laid` where there is one
/// ([`LaidOut`]), and otherwise as it is.
fn laid_out(
    element: impl IntoElement,
    laid: Option<&std::rc::Rc<std::cell::Cell<Option<LayoutId>>>>,
) -> AnyElement {
    match laid {
        Some(laid) => LaidOut {
            child: element.into_any_element(),
            laid: laid.clone(),
        }
        .into_any_element(),
        None => element.into_any_element(),
    }
}
/// The body's content in the frame that first draws a failure or a refusal a send ended in,
/// on the top line among the content ([`Panel::show_line`]): the body shows the failure's
/// line, with its action where it has one (`line`), and with Refresh PRs where focus rests
/// on it as the line's remedy (`remedy`), before its content prepaints, so this frame draws
/// them where they stay, with the focus the landing gave, and no later frame moves them: the
/// first frame after a change lays out its final state (DESIGN.md, Motion). The reveal once
/// ran after that frame had painted ([`review::show`]): the frame drew the line above the
/// view and the form below it moved down by the line's height, about five lines at 18 pt in a
/// 461 × 490 window, and the next frame jumped to the line. Made for that frame alone: every
/// other frame's content is drawn as it is, and reads one flag.
struct Failed {
    content: AnyElement,
    body: ScrollHandle,
    blocks: review::Blocks,
    revealed: std::rc::Rc<std::cell::RefCell<review::FocusObservation>>,
    line: std::rc::Rc<std::cell::Cell<Option<LayoutId>>>,
    remedy: std::rc::Rc<std::cell::Cell<Option<LayoutId>>>,
}
impl Failed {
    /// How far the body moves its content to show the line whole, a line's room clear of its
    /// edges, by the least movement, having moved its scroll by as much, if it moves
    /// ([`review::show_laid_out`]). A line taller than the view shows from its top, where its
    /// first lines and the top of its ring are.
    fn show(&self, window: &mut Window, cx: &App) -> Option<Pixels> {
        let mut shown = window.layout_bounds(self.line.get()?);
        if let Some(remedy) = self.remedy.get() {
            shown = shown.union(&window.layout_bounds(remedy));
        }
        let inset = review::reveal_room(window, cx);
        review::show_laid_out(&self.body, &self.blocks, shown, inset, cx)
    }
}
impl IntoElement for Failed {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for Failed {
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
        (self.content.request_layout(window, cx), ())
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
        let moved = self.show(window, cx);
        // The landing has shown the focus it gave: the body's parts, which reveal a focus
        // new to them once the frame has painted, show nothing anew, so a line taller than
        // the view stays where this frame put it.
        *self.revealed.borrow_mut() = review::FocusObservation {
            focus: window.focused(cx),
            viewport: self.body.bounds(),
            rem_size: window.rem_size(),
        };
        // Content the body moves is prepainted where its new offset puts it.
        match moved {
            Some(moved) => window.with_element_offset(point(Pixels::ZERO, moved), |window| {
                self.content.prepaint(window, cx);
            }),
            None => {
                self.content.prepaint(window, cx);
            }
        }
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
        self.content.paint(window, cx);
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
pub(crate) fn dialog_chrome(window: &Window) -> Pixels {
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
        .debug_selector(move || selector)
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
    /// Whether the panel's New pull request form holds text the draft store does not
    /// ([`Panel::holds_unsaved`]), which the closed panel alone keeps.
    unsaved: bool,
}
impl WarmPanels {
    /// Keeps `entry`'s closed panel, in place of an earlier one of its window and worktree,
    /// within the cache's bounds: eight panels and 32 MiB of counted payload, past which the
    /// panel closed longest ago goes, of those whose form's text the store holds. One whose
    /// form holds text saved nowhere stays, counted like any other: its status told the
    /// person that keeping the window open keeps that text, and going with the ninth panel
    /// closed after it the text was lost in an open window. Each such panel took typing to
    /// make, and what it holds beyond the bounds is its own payload.
    fn retain(&mut self, entry: WarmPanel) {
        self.0
            .retain(|old| old.owner != entry.owner || old.worktree != entry.worktree);
        self.0.push(entry);
        while self.0.len() > 8
            || self.0.iter().map(|entry| entry.bytes).sum::<usize>() > 32 * 1024 * 1024
        {
            let Some(oldest) = self.0.iter().position(|entry| !entry.unsaved) else {
                break;
            };
            self.0.remove(oldest);
        }
    }
}
/// A press of Pull requests that could not open the panel
/// ([`GitTurtle::github_open_refusal`]), which the status bar says while it is fresh.
#[derive(Default)]
pub(super) struct OpenRefusal {
    /// How many presses have been refused, so each is announced again with a fresh node.
    turns: usize,
    note: Option<OpenRefusalNote>,
    /// Where Pull requests was last laid out, full or compact, which a keyboard press's answer
    /// stands beside as its tooltip does ([`GitTurtle::github_refusal_announcement`]), read as
    /// the answer is prepainted, after the button in the same frame.
    button: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>,
    /// Where the header's and the tab strip's other controls were prepainted in the frame being
    /// drawn, each where its clip leaves it shown, which the reason's popup at Pull requests
    /// stands clear of ([`pull_requests_answer_side`]): the return action, the repository
    /// identity, History and Changes, a running write's status and Cancel, Projects, the
    /// profile and Settings, each repository tab with its close button, Menu, Open repository
    /// tab and Workspaces ([`GitTurtle::header_control`]). Forgotten as each frame's tab strip,
    /// the first of those rows, is built ([`GitTurtle::begin_header_controls`]).
    controls: std::rc::Rc<std::cell::RefCell<Vec<Bounds<Pixels>>>>,
    /// [`Self::controls`] as the frame before recorded them, which decides ahead of their
    /// recording whether a frame lays out the reason's popup to choose its side
    /// ([`PlacedAnswer`]).
    last_controls: std::rc::Rc<std::cell::RefCell<Vec<Bounds<Pixels>>>>,
    /// The side of Pull requests the reason's popup took as it was last prepainted while the
    /// panel was refused, which the pointer's tooltip takes there ([`PlacedAnswer`]); none
    /// where nothing refuses the panel. The kit reads it as it places that tooltip, later in
    /// the same frame (`Button::tooltip_view_at`).
    side: std::rc::Rc<std::cell::Cell<Option<gpui_kit::base::Placement>>>,
    /// While the panel is refused and no keyboard press's answer is kept, the reason's popup,
    /// never drawn, laid out to choose the side the pointer's tooltip takes in a frame whose
    /// inputs to that side changed ([`AnswerSide`]); built once for each reason.
    measure: std::cell::RefCell<Option<KeptAnswer>>,
    /// The side the reason's popup last took, with what it was chosen from, kept for the
    /// window, so a frame that changes none of that lays out no popup to choose it again and
    /// asks for no frame more ([`PlacedAnswer`]).
    placed: std::rc::Rc<std::cell::RefCell<Option<AnswerSide>>>,
    /// Whether the last frame drawn already showed a keyboard press's answer at the button: a
    /// pointer hover that starts on Frame 2+ of an answer stays suppressed even after the write
    /// ends, without detaching the trigger's tooltip (which would drop `TooltipLifetime` and tear
    /// down a pointer resting since Frame 1).
    answered: std::rc::Rc<std::cell::Cell<bool>>,
    /// Whether the kit's tooltip overlay built the button's pointer tooltip in the last frame
    /// drawn as the empty view it builds for a request the button made while a keyboard press's
    /// answer stood: the overlay then held the button's own request, and no other control's,
    /// which the button's prepaint later in that frame hides unless the pointer rests on the
    /// button ([`GitTurtle::refusing_pull_requests`]). Cleared as the header is built in each
    /// frame, before the overlay builds, so it reflects the last frame drawn alone, and only
    /// that frame's prepaint reads it: the answer's end, between frames, reads none of it, so a
    /// neighbour's hover that took the overlay since is never hidden for a request it no
    /// longer holds.
    built_empty: std::rc::Rc<std::cell::Cell<bool>>,
    /// Whether the empty build came from a hover that started after the answer's first frame, so
    /// the button's prepaint hides the overlay even while the pointer rests on the button.
    built_suppressed: std::rc::Rc<std::cell::Cell<bool>>,
    /// The key of a keyboard press of Pull requests the panel refused as it went down, until
    /// it comes up: its repeats answer nothing more and its key-up presses nothing, also once
    /// the write has ended ([`GitTurtle::pull_requests_key_down`]). This latch is the
    /// button's own, not the kit's: a key-up lost while focus and the window's activation
    /// stay as they were, as X11 drops one within 20 ms of the next press, leaves it set, and
    /// that key's next press then opens nothing, which a second press does; it never makes a
    /// write. Focus leaving the button and the window turning inactive clear it.
    held: Option<&'static str>,
    /// For a press by Return or Space, the tooltip that answers it at the button, as the
    /// pointer's tooltip answers a click there, for as long as the button keeps focus and the
    /// panel stays refused ([`GitTurtle::answer_pull_requests_at_button`]).
    at_button: Option<KeptAnswer>,
    /// Takes the answer at the button, and the held press, away as focus leaves the button.
    _blur: Option<Subscription>,
    /// Builds the answer at the button again as the reason the panel is refused changes, and
    /// takes it away as what refused the panel ends.
    _spell: Option<Subscription>,
}
/// The answer a keyboard press of Pull requests stands at the button, and the reason it says.
struct KeptAnswer {
    reason: &'static str,
    view: AnyView,
}
struct OpenRefusalNote {
    turn: usize,
    /// The reason the press was refused for, which another reason refusing the panel, or none,
    /// drops the note for ([`GitTurtle::say_github_open_refused`]).
    cause: &'static str,
    /// What it says, in the words the button's tooltip gives ([`open_refused_sentence`]).
    reason: SharedString,
    _expiry: Task<()>,
    /// Drops the note as the panel's refusal changes from `cause`.
    _watch: Subscription,
}
/// The tooltip Pull requests gives, `text`, as the pointer's tooltip and a keyboard press's
/// answer draw it, its text drawn as `pull-requests/tooltip/‹text›` for tests.
fn pull_requests_tooltip(text: SharedString, cx: &mut App) -> AnyView {
    cx.new(|_| {
        Tooltip::element(move |_, _| {
            let probe = text.clone();
            div()
                .debug_selector(move || format!("pull-requests/tooltip/{probe}"))
                .child(text.clone())
        })
    })
    .into()
}
/// Whether the pointer rests on Pull requests, laid out at `bounds`: inside it, and the last
/// input the pointer's, since GPUI takes a key press as ending a hover until the pointer moves.
fn pointer_rests(window: &Window, bounds: Bounds<Pixels>) -> bool {
    !window.last_input_was_keyboard() && bounds.contains(&window.mouse_position())
}
/// The kit tooltip's margin around its popup inside the element its positioner places, in rems
/// (`POPUP_MARGIN` in the kit's `tooltip.rs`, private there): the popup's drawn edge, its 1 px
/// border, stands this far inside that element.
const TOOLTIP_POPUP_MARGIN: f32 = 0.75;
/// The side of Pull requests, laid out at `button`, on which the reason's popup, the element of
/// `size` the kit's tooltip positioner places beside the button as it places the pointer's
/// tooltip, centred on it and clamped into the window's 4 px margin, covers the least of the
/// other controls of the header and the tab strip, `controls`, and of the button itself: above
/// where its drawn edge covers none of them, else below where it covers none, else the side
/// where it covers less of them, by area. Its drawn edge stands the popup's margin inside the
/// element, and the button's focus ring's room clear of the ring above or below it. Placed
/// above wherever it fitted, at 13 pt in a 1,480 × 973 window the answer covered the tab strip
/// over the repository tabs, and in a 461 × 490 one the Changes tab too, so their names were
/// lost for the whole write.
fn pull_requests_answer_side(
    button: Bounds<Pixels>,
    controls: &[Bounds<Pixels>],
    size: Size<Pixels>,
    window: &Window,
) -> gpui_kit::base::Placement {
    use gpui_kit::base::Placement;
    let inset = window.rem_size() * TOOLTIP_POPUP_MARGIN;
    // The positioner's margin, outside the client inset of client-side decorations.
    let margin = px(4.) + window.client_inset().unwrap_or(px(0.));
    let viewport = window.viewport_size();
    let (right, foot) = (
        (viewport.width - margin).max(margin),
        (viewport.height - margin).max(margin),
    );
    let covered = |placement: Placement| -> f32 {
        let y = match placement {
            Placement::Top => button.top() - size.height,
            _ => button.bottom(),
        };
        let mut placed = Bounds::new(point(button.center().x - size.width / 2., y), size);
        if placed.right() > right {
            placed.origin.x -= placed.right() - right;
        }
        placed.origin.x = placed.origin.x.max(margin);
        if placed.bottom() > foot {
            placed.origin.y -= placed.bottom() - foot;
        }
        placed.origin.y = placed.origin.y.max(margin);
        let drawn = Bounds::from_corners(
            placed.origin + point(inset, inset),
            placed.bottom_right() - point(inset, inset),
        );
        controls
            .iter()
            .chain([&button])
            .map(|control| {
                let overlap = drawn.intersect(control);
                let (width, height) = (
                    f32::from(overlap.size.width),
                    f32::from(overlap.size.height),
                );
                // A popup that meets a control's edge on a pixel's rounding covers nothing.
                if width > 0.5 && height > 0.5 {
                    width * height
                } else {
                    0.
                }
            })
            .sum()
    };
    if covered(Placement::Bottom) < covered(Placement::Top) {
        Placement::Bottom
    } else {
        Placement::Top
    }
}
/// What the side of Pull requests the reason's popup takes is chosen from
/// ([`pull_requests_answer_side`]): the reason, whose sentence sizes the popup, where the button
/// and the header's and the tab strip's other controls lie in the frame, the interface font and
/// its sizes, and the window's scale, size and client inset; with the side they gave. A frame
/// that changes none of them takes that side again without laying the popup
/// out: chosen in every frame, a write's steady frames each laid out a tooltip view never drawn.
struct AnswerSide {
    reason: &'static str,
    button: Bounds<Pixels>,
    controls: Vec<Bounds<Pixels>>,
    font: SharedString,
    font_size: Pixels,
    rem: Pixels,
    scale: f32,
    viewport: Size<Pixels>,
    inset: Option<Pixels>,
    side: gpui_kit::base::Placement,
}
impl AnswerSide {
    /// Whether the side was chosen from what this frame would choose it from.
    fn holds(
        &self,
        reason: &'static str,
        button: Bounds<Pixels>,
        controls: &[Bounds<Pixels>],
        window: &Window,
        cx: &App,
    ) -> bool {
        let theme = gpui_kit::component::Theme::global(cx);
        self.reason == reason
            && self.button == button
            && self.controls == controls
            && self.font == theme.font_family
            && self.font_size == theme.font_size
            && self.rem == window.rem_size()
            && self.scale == window.scale_factor()
            && self.viewport == window.viewport_size()
            && self.inset == window.client_inset()
    }
}
#[cfg(test)]
thread_local! {
    /// How many times the reason's popup at Pull requests was laid out to choose its side.
    static ANSWER_SIDE_LAYOUTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
/// The reason's popup at Pull requests, placed on the side [`pull_requests_answer_side`] takes in
/// its own prepaint, which runs, as a deferred draw's does, once the rest of the frame has been
/// prepainted, so the button and the other controls have recorded where this frame lays them
/// out: a side chosen from the frame before could swing back and forth as the popup's side
/// moved what it was weighed against. Drawn where a keyboard press's answer is kept, by the
/// positioner the kit places the pointer's tooltip with, so the two stand on the same pixels;
/// otherwise laid out only, to choose the side the pointer's tooltip takes, which the kit's tooltip
/// overlay, registered after it at the same priority, reads as it places that tooltip later in
/// the same frame, so the frame that changes the side draws the tooltip there, also one shown
/// since before the panel was refused. The side is chosen again only in a frame that changes
/// what it is chosen from ([`AnswerSide`]); in any other the popup, unless drawn, is not laid
/// out at all.
struct PlacedAnswer {
    /// The popup, until it is laid out inside [`Self::positioned`].
    popup: Option<AnyElement>,
    /// The popup inside the kit's tooltip positioner: where it is drawn, the positioner the
    /// kit's tooltip overlay places the pointer's tooltip with, beside the button as this frame
    /// lays it out and on the side chosen, laid out and placed in this element's prepaint as
    /// the overlay's is, so the two stand at the same place by construction; otherwise laid
    /// out with the frame to size it alone, where the side is chosen again.
    positioned: Option<AnyElement>,
    /// The reason it says.
    reason: &'static str,
    button: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>,
    controls: std::rc::Rc<std::cell::RefCell<Vec<Bounds<Pixels>>>>,
    /// Where they lay in the frame before, which the popup's layout, ahead of their recording
    /// in this frame, is decided from.
    last_controls: std::rc::Rc<std::cell::RefCell<Vec<Bounds<Pixels>>>>,
    side: std::rc::Rc<std::cell::Cell<Option<gpui_kit::base::Placement>>>,
    placed: std::rc::Rc<std::cell::RefCell<Option<AnswerSide>>>,
    drawn: bool,
}
impl IntoElement for PlacedAnswer {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for PlacedAnswer {
    /// Whether the popup was laid out with the rest of the frame.
    type RequestLayoutState = bool;
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&gpui_kit::GlobalElementId>,
        _: Option<&gpui_kit::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (gpui_kit::LayoutId, bool) {
        // Laid out with the rest of the frame, before this frame's controls record where they
        // lie: a popup not drawn is left out where the side held for the last frame's.
        let held = self.placed.borrow().as_ref().is_some_and(|placed| {
            placed.holds(
                self.reason,
                self.button.get(),
                &self.last_controls.borrow(),
                window,
                cx,
            )
        });
        // A drawn popup is laid out as it is placed, once the button has recorded where this
        // frame lays it out. Its node stands out of the flow, as the positioner does, so it
        // takes no room and no gap among the status bar's parts it is drawn among: in the flow,
        // it took a gap from the status line in each of a write's steady frames.
        if held || self.drawn {
            return (
                window.request_layout(
                    gpui::Style {
                        position: gpui::Position::Absolute,
                        ..gpui::Style::default()
                    },
                    None,
                    cx,
                ),
                false,
            );
        }
        let mut sized = gpui_kit::base::TooltipPositioner::new(self.button.get())
            .children(self.popup.take())
            .into_any_element();
        let laid = sized.request_layout(window, cx);
        self.positioned = Some(sized);
        (laid, true)
    }
    fn prepaint(
        &mut self,
        _: Option<&gpui_kit::GlobalElementId>,
        _: Option<&gpui_kit::InspectorElementId>,
        bounds: Bounds<Pixels>,
        laid_out: &mut bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        let button = self.button.get();
        let held = self.placed.borrow().as_ref().and_then(|placed| {
            placed
                .holds(self.reason, button, &self.controls.borrow(), window, cx)
                .then_some(placed.side)
        });
        // The drawn popup in the positioner the kit places the pointer's tooltip with, which
        // reads the side as it is placed below, laid out as the overlay lays its own out.
        let drawn = self.drawn.then(|| {
            let mut positioned = gpui_kit::base::TooltipPositioner::new(button)
                .placement_following(self.side.clone())
                .children(self.popup.take())
                .into_any_element();
            let size = positioned.layout_as_root(
                window.viewport_size().map(gpui::AvailableSpace::Definite),
                window,
                cx,
            );
            (positioned, size)
        });
        let side = match held {
            Some(side) => side,
            None => {
                // This frame's controls moved since the last: a popup left out is laid out now,
                // to choose from; one not drawn is never placed, so where it would stand does
                // not matter.
                let size = match (&drawn, *laid_out) {
                    (Some((_, size)), _) => *size,
                    (None, true) => bounds.size,
                    (None, false) => gpui_kit::base::TooltipPositioner::new(button)
                        .children(self.popup.take())
                        .into_any_element()
                        .layout_as_root(gpui::AvailableSpace::min_size(), window, cx),
                };
                let controls = self.controls.borrow();
                let side = pull_requests_answer_side(button, &controls, size, window);
                #[cfg(test)]
                ANSWER_SIDE_LAYOUTS.with(|layouts| layouts.set(layouts.get() + 1));
                let theme = gpui_kit::component::Theme::global(cx);
                *self.placed.borrow_mut() = Some(AnswerSide {
                    reason: self.reason,
                    button,
                    controls: controls.clone(),
                    font: theme.font_family.clone(),
                    font_size: theme.font_size,
                    rem: window.rem_size(),
                    scale: window.scale_factor(),
                    viewport: window.viewport_size(),
                    inset: window.client_inset(),
                    side,
                });
                side
            }
        };
        self.side.set(Some(side));
        // Laid out where the kit's tooltip overlay lays its positioner out, in an absolute inset-0
        // layer at the window's origin: the positioner moves the popup by whole pixels from there,
        // so the two stand on the same pixels.
        let origin = Point::default();
        self.positioned = drawn.map(|(mut positioned, _)| {
            positioned.prepaint_at(origin, window, cx);
            positioned
        });
    }
    fn paint(
        &mut self,
        _: Option<&gpui_kit::GlobalElementId>,
        _: Option<&gpui_kit::InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut bool,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        if self.drawn
            && let Some(positioned) = &mut self.positioned
        {
            positioned.paint(window, cx);
        }
    }
}
/// What a refused press of Pull requests says, in its tooltip, the status line and the live
/// region alike: the panel's name as its subject, so it means something heard alone, and what
/// it waits for, without implying the panel will open by itself afterwards.
pub(super) fn open_refused_sentence(reason: &str) -> SharedString {
    format!(
        "Pull requests: {}{}",
        reason[..1].to_ascii_lowercase(),
        &reason[1..]
    )
    .into()
}
impl GitTurtle {
    /// Why the panel cannot open now, or none where it can. A read of the panel's own that
    /// closing cancelled holds the window's busy spell until the executor has stopped it, so
    /// it does not refuse the panel, which opens at once with its own reads waiting behind
    /// that drain ([`Panel::load_local`]) and says why its requests wait meanwhile. A request
    /// that may have changed something, an outbound action or an account change, and any Git
    /// write or preparation still refuse it.
    pub(super) fn github_open_refusal(&self) -> Option<&'static str> {
        let busy = self.operation_busy?;
        if running_request(busy).is_some_and(RequestKind::reads_only) {
            return None;
        }
        Some(self.busy_reason())
    }

    /// The header's Pull requests as the window's busy spell leaves it: while the panel cannot
    /// open it is announced unavailable and its tooltip says why, from the spell's first frame
    /// to its last, and once the spell looks busy its label takes the disabled colour. It is
    /// never `.disabled`, which `workspace::refusing` would make it: the kit's disabled Button
    /// drops a click and Enter and leaves the Tab order, so 200 ms into a Fetch or a Push a
    /// press went unanswered ([`Self::say_github_open_refused`]) for the rest of the spell. The
    /// colour is the label's alone, so the focus ring stays whole. A refused press also answers
    /// on the button itself: the tooltip says the reason ([`open_refused_sentence`]) and stays
    /// through the press, since the status line is at the window's foot, 900 px from a pointer
    /// on the header, and Return or Space on the focused button stand the same tooltip there
    /// for the rest of the spell ([`Self::pull_requests_key_down`]). At rest a press opens the
    /// panel, and the tooltip goes with it. `compact` is the short header form, whose tooltip
    /// names the control; the tooltip is formatted only as it shows, so a frame copies none
    /// of it.
    ///
    /// While a keyboard press's answer is kept the button offers no pointer tooltip, which
    /// would say the same sentence over it: the answer stays drawn under a pointer that rests
    /// on the button, crosses it or leaves it. The answer's first frame still offers it, so
    /// GPUI, which tells a hover only to a listener drawn, tells the button the key press ended
    /// any hover it last reported, and none is left standing to end once the answer has gone,
    /// hiding whatever tooltip the overlay then holds, a neighbour's among them. A pointer that
    /// meets the button before the next frame asks for a tooltip that GPUI then never tells
    /// the pointer has left: whenever the overlay builds it, while the answer stands or once
    /// it has gone, it draws nothing, and the button's prepaint in the same frame hides it,
    /// unless the pointer rests on the button, where once the answer has gone the button's
    /// own tooltip is built in its place, so no tooltip is drawn at a button the pointer has
    /// left, a show still waiting out its delay as the answer ends included, and the next
    /// hover elsewhere waits out its own delay. The kit's overlay is hidden from here only
    /// while it holds the button's request, never another control's. Offered from the answer's
    /// second frame, an empty tooltip under a pointer resting on the button left the overlay
    /// holding the button's request, and a hover on another control took its tooltip at once,
    /// without the show delay, sliding from Pull requests.
    ///
    /// While the panel is refused the pointer's tooltip stands on the side of the button the
    /// reason's popup takes as it is placed in the same frame ([`PlacedAnswer`]), clear of the
    /// header's and the tab strip's other controls where a side leaves them clear, in every
    /// frame it is drawn, a hover that began before the refusal or before the side changed
    /// included: the kit reads the side as it places the tooltip, after the popup has chosen
    /// it, never as the hover begins. Read as the hover began, a tooltip kept the side of that
    /// frame for the whole write, over the tab strip's tabs where the answer stood below, and
    /// even a hover that began during the refusal took the side a frame late.
    pub(super) fn refusing_pull_requests(
        &self,
        open: Button,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> Button {
        let reason = self.github_open_refusal();
        let refused = reason.is_some();
        // The reason's popup, measured to choose the pointer's side while no answer is kept.
        {
            let mut measure = self.github_refusal.measure.borrow_mut();
            match reason.filter(|_| self.github_refusal_kept().is_none()) {
                Some(reason) if measure.as_ref().is_some_and(|kept| kept.reason == reason) => {}
                Some(reason) => {
                    *measure = Some(KeptAnswer {
                        reason,
                        view: pull_requests_tooltip(open_refused_sentence(reason), cx),
                    })
                }
                None => *measure = None,
            }
        }
        if !refused {
            self.github_refusal.side.set(None);
        }
        let side = self.github_refusal.side.clone();
        let dimmed = refused && self.operation_shown().is_some();
        #[cfg(test)]
        crate::workspace::refusals::record("pull-requests".into(), refused, dimmed);
        let idle = if compact {
            "Pull requests · Discover pull requests and review captured changes"
        } else {
            "Discover pull requests and review captured changes"
        };
        let owner = cx.entity().downgrade();
        // Where it lies, for the answer a keyboard press gets beside it, which the answer reads
        // as it is placed later in the same frame. Keep the trigger's tooltip attached while an
        // answer stands so `TooltipLifetime` never tears down a pointer resting since Frame 1; a
        // hover that begins on Frame 2+ captures `suppressed = true` and yields an empty view that
        // prepaint hides even while the pointer rests on the button.
        let (laid, answering, answered) = (
            self.github_refusal.button.clone(),
            self.github_refusal_kept().is_some(),
            self.github_refusal.answered.clone(),
        );
        let suppressed = answering && answered.get();
        let built_empty = self.github_refusal.built_empty.clone();
        let built_suppressed = self.github_refusal.built_suppressed.clone();
        built_empty.set(false);
        built_suppressed.set(false);
        let (resting, held, hide_suppressed) =
            (laid.clone(), built_empty.clone(), built_suppressed.clone());
        let open = gpui_kit::base::ElementExt::on_prepaint(open, move |bounds, window, cx| {
            laid.set(bounds);
            answered.set(answering);
            // The overlay built the button's request, made while an answer stood, as nothing in
            // this frame, either under a pointer that has left the button or from a hover that
            // started after the answer's first frame: hand the overlay back.
            if hide_suppressed.get() || (held.get() && !pointer_rests(window, bounds)) {
                gpui_kit::component::hide_tooltip(window, cx);
            }
        });
        open.accessibility_disabled(refused)
            .when(dimmed, |open| {
                open.text_color(gpui_kit::component::Theme::global(cx).disabled_foreground())
            })
            .when(refused, |open| open.tooltip_through_press())
            .capture_key_down(cx.listener(Self::pull_requests_key_down))
            .capture_key_up(cx.listener(Self::pull_requests_key_up))
            .on_action(cx.listener(Self::pull_requests_escape))
            .tooltip_view_at(side, move |window, cx| {
                let (drawn, kept, refusal) = owner.upgrade().map_or((false, false, None), |app| {
                    let app = app.read(cx);
                    (
                        app.page == AppPage::Repository && app.repository.is_some(),
                        app.github_refusal_kept().is_some(),
                        app.github_open_refusal(),
                    )
                });
                // The header draws the button only on the Repository page, where its
                // prepaint keeps `answering` and `resting` for this frame: elsewhere they
                // are the last Repository frame's, and a request still held, under a
                // pointer in the button's old place, drew the reason over Settings and the
                // idle sentence once the write ended. A page change hides the overlay too
                // (`GitTurtle::hide_tooltip_on_page_change`).
                if !drawn {
                    return cx.new(|_| gpui::EmptyView).into();
                }
                // A pointer that met the button after the answer's first frame stays suppressed
                // even after the write ends; one that met it in the first frame draws nothing
                // beside the answer, and once the answer has gone, nothing if it has left since.
                if suppressed {
                    built_empty.set(true);
                    built_suppressed.set(true);
                    return cx.new(|_| gpui::EmptyView).into();
                }
                if kept || (answering && !pointer_rests(window, resting.get())) {
                    built_empty.set(true);
                    return cx.new(|_| gpui::EmptyView).into();
                }
                let text = refusal.map_or(SharedString::new_static(idle), open_refused_sentence);
                pull_requests_tooltip(text, cx)
            })
    }

    /// Return or Space going down on the focused Pull requests while the panel cannot open is
    /// answered at once and at the button, as a click is under the pointer
    /// ([`Self::answer_pull_requests_at_button`]), once for the press: its repeats, marked held
    /// or, under X11, sent as fresh presses with no key-up between them, answer nothing more.
    /// The button's own press, which GPUI arms on any Enter or Space key-down and makes as the
    /// key comes up, is never armed for it, so a press held until the write has ended opens
    /// nothing as it comes up. Any other key ends the press, as it ends a held key's repeats.
    fn pull_requests_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        if self.github_refusal.held == Some(key) {
            window.prevent_default();
            return;
        }
        self.github_refusal.held = None;
        let key = match key {
            "enter" => "enter",
            "space" => "space",
            _ => return,
        };
        if event.keystroke.modifiers.modified() || self.page != AppPage::Repository {
            return;
        }
        let Some(reason) = self.github_open_refusal() else {
            return;
        };
        window.prevent_default();
        self.github_refusal.held = Some(key);
        self.answer_pull_requests_at_button(reason, window, cx);
    }

    /// The key-up that ends a press [`Self::pull_requests_key_down`] answered presses nothing.
    fn pull_requests_key_up(
        &mut self,
        event: &KeyUpEvent,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        if self.github_refusal.held == Some(event.keystroke.key.as_str()) {
            self.github_refusal.held = None;
            window.prevent_default();
        }
    }

    /// Escape on the focused Pull requests takes away the answer a keyboard press stands at
    /// the button ([`Self::answer_pull_requests_at_button`]), and does nothing else while it
    /// is drawn, wherever the pointer rests; otherwise it goes on to the window's Escape.
    /// Escape is bound across the window
    /// ([`crate::ClearSearch`]), and GPUI gives a key's binding the press before any key-down
    /// listener, so [`Self::pull_requests_key_down`] never hears it.
    fn pull_requests_escape(
        &mut self,
        _: &crate::ClearSearch,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.github_refusal_kept().is_none() {
            cx.propagate();
            return;
        }
        self.end_pull_requests_answer(cx);
    }

    /// Answers a keyboard press of Pull requests the panel refuses as the pointer's tooltip
    /// answers a click: the reason stands at the button in its tooltip, said again as the
    /// reason changes, until what refused the panel ends, focus leaves the button or Escape;
    /// the status line says it for [`crate::copy_feedback::RESTORE_AFTER`]
    /// ([`Self::say_github_open_refused`]), and its live region speaks it once for the press,
    /// so the tooltip is drawn out of the accessibility tree. The press hides any tooltip the
    /// pointer brought, as a key press ends a pointer's hover, and the button offers its own
    /// again only once the answer has gone ([`Self::refusing_pull_requests`]), so the two
    /// never stand one over the other. The press once answered only in the status line,
    /// 900 px from the button, and then at the button only as long as the status line, too
    /// briefly to read.
    fn answer_pull_requests_at_button(
        &mut self,
        reason: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.say_github_open_refused(reason, cx);
        gpui_kit::component::hide_tooltip(window, cx);
        self.github_refusal.at_button = Some(KeptAnswer {
            reason,
            view: pull_requests_tooltip(open_refused_sentence(reason), cx),
        });
        self.github_refusal._blur =
            Some(
                cx.on_blur(&self.pull_requests_focus, window, |this, _, cx| {
                    this.github_refusal.held = None;
                    this.end_pull_requests_answer(cx);
                }),
            );
        let app = cx.entity();
        self.github_refusal._spell = Some(cx.observe_in(&app, window, |this, _, window, cx| {
            this.follow_pull_requests_refusal(window, cx)
        }));
    }

    /// Keeps the answer at Pull requests saying why the panel is refused: built again for a
    /// reason that changed, as a write gives way to an outbound action, and taken away once
    /// nothing refuses the panel. It once kept the sentence of the spell it began in.
    fn follow_pull_requests_refusal(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(reason) = self.github_open_refusal() else {
            self.end_pull_requests_answer(cx);
            return;
        };
        if let Some(kept) = self.github_refusal.at_button.as_mut()
            && kept.reason != reason
        {
            *kept = KeptAnswer {
                reason,
                view: pull_requests_tooltip(open_refused_sentence(reason), cx),
            };
            cx.notify();
        }
    }

    /// Takes away the answer a keyboard press stands at the button, if any. The kit's overlay is
    /// left as it is: a request the button made while the answer stood is built as nothing and
    /// hidden in the next frame the overlay builds it under a pointer that has left the button
    /// ([`Self::refusing_pull_requests`]), and under a pointer resting there it becomes the
    /// button's own tooltip. Hidden here while the overlay had last built the button's request,
    /// a pointer resting on the button lost its tooltip as the write ended, and a show still
    /// waiting out its delay drew the button's tooltip at a button the pointer had left.
    fn end_pull_requests_answer(&mut self, cx: &mut Context<Self>) {
        self.github_refusal._spell = None;
        if self.github_refusal.at_button.take().is_some() {
            cx.notify();
        }
    }

    /// The answer a keyboard press of Pull requests stands at the button while the panel is
    /// still refused ([`Self::answer_pull_requests_at_button`]), drawn wherever the pointer
    /// rests, in the words of the reason that refuses it now.
    fn github_refusal_kept(&self) -> Option<&AnyView> {
        self.github_refusal
            .at_button
            .as_ref()
            .filter(|_| self.github_open_refusal().is_some())
            .map(|kept| &kept.view)
    }

    /// Says why a press could not open the panel, as the status line's text while it is
    /// fresh, for [`crate::copy_feedback::RESTORE_AFTER`], as a write's outcome is, and in the
    /// button's tooltip. Nothing else on screen changes.
    fn say_github_open_refused(&mut self, reason: &'static str, cx: &mut Context<Self>) {
        self.github_refusal.turns = self.github_refusal.turns.wrapping_add(1);
        let expiry = cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(crate::copy_feedback::RESTORE_AFTER)
                .await;
            let _ = this.update(cx, |this, cx| {
                this.github_refusal.note = None;
                cx.notify();
            });
        });
        // Dropped, not set aside, as the refusal changes: a reason going back to this one
        // within the note's time once brought the first press's sentence back.
        let watch = cx.observe(&cx.entity(), |this, _, cx| {
            if let Some(note) = &this.github_refusal.note
                && this.github_open_refusal() != Some(note.cause)
            {
                this.github_refusal.note = None;
                cx.notify();
            }
        });
        self.github_refusal.note = Some(OpenRefusalNote {
            turn: self.github_refusal.turns,
            cause: reason,
            reason: open_refused_sentence(reason),
            _expiry: expiry,
            _watch: watch,
        });
        cx.notify();
    }

    /// The note a refused press leaves while it is fresh, dropped, not renamed, once another
    /// reason refuses the panel, as an outbound action follows a write, or none does: the press
    /// was refused for the reason it says ([`Self::say_github_open_refused`]). Kept, it went on
    /// saying the write's for up to [`crate::copy_feedback::RESTORE_AFTER`] while the answer at
    /// the button said the action's; set aside instead, it came back as the write's reason did
    /// within that time, and while nothing refused the panel its live region stayed drawn.
    fn github_refusal_note(&self) -> Option<&OpenRefusalNote> {
        self.github_refusal.note.as_ref()
    }

    /// The refusal the status line reads while it is fresh and the panel is still refused: a
    /// read's error and a write's fresh outcome keep the line, and it is announced beside them
    /// ([`Self::github_refusal_announcement`]). A "Wait for…" could otherwise stay up for
    /// [`crate::copy_feedback::RESTORE_AFTER`] after the write it waited for had ended.
    pub(super) fn github_refusal_line(&self) -> Option<&str> {
        let note = self.github_refusal_note()?;
        (self.page == AppPage::Repository
            && self.error.is_none()
            && self.fresh_outcome().is_none()
            && self.github_open_refusal().is_some())
        .then_some(note.reason.as_ref())
    }

    /// The status live region that says why a press could not open the panel, a fresh node
    /// for each press, as the narrow rail's refusal is: nothing else on screen changes, and
    /// a repeated refusal would otherwise go unheard. Its text is the node's name and its
    /// value, which AT-SPI and macOS announce respectively. It draws nothing. Beside it, out
    /// of the button's node, whose name it would otherwise join, stands the tooltip that
    /// answers a keyboard press at the button ([`Self::answer_pull_requests_at_button`]),
    /// placed by the positioner the kit places the pointer's with, on the side of the button
    /// [`pull_requests_answer_side`] takes ([`PlacedAnswer`]), and left out
    /// of the accessibility tree with all it holds, the kit's tooltip node among them: the
    /// live region has spoken it, and a reader that reached it would say it a second time.
    pub(super) fn github_refusal_announcement(&self) -> Vec<AnyElement> {
        /// The kit's tooltip overlay's priority, so the answer stands where the pointer's would.
        const TOOLTIP_PRIORITY: usize = 200;
        let mut drawn = Vec::new();
        if let Some(note) = self.github_refusal_note() {
            drawn.push(
                crate::native_accessibility::recorded(
                    div()
                        .id(("github-open-refused", note.turn))
                        .debug_selector(|| "github-open-refused".into())
                        .role(Role::Status)
                        .polite()
                        .aria_label(note.reason.clone())
                        .aria_value(note.reason.clone())
                        .absolute()
                        .size(px(1.))
                        .overflow_hidden(),
                )
                .into_any_element(),
            );
        }
        let kept = self.github_refusal_kept().cloned();
        let measure = self.github_refusal.measure.borrow();
        let measured = self.github_open_refusal().and_then(|reason| {
            measure
                .as_ref()
                .filter(|measured| measured.reason == reason)
                .map(|measured| measured.view.clone())
        });
        let reason = self.github_open_refusal();
        let popup = match (kept, measured) {
            (Some(answer), _) => Some((
                crate::native_accessibility::hidden_from_assistive_technology(
                    div()
                        .id("pull-requests-answer")
                        .debug_selector(|| "pull-requests-answer".into())
                        .child(answer),
                    true,
                )
                .into_any_element(),
                true,
            )),
            (None, Some(measured)) => Some((div().child(measured).into_any_element(), false)),
            (None, None) => None,
        };
        if let (Some((popup, kept)), Some(reason)) = (
            popup.filter(|_| self.page == AppPage::Repository && self.repository.is_some()),
            reason,
        ) {
            drawn.push(
                deferred(PlacedAnswer {
                    popup: Some(popup),
                    positioned: None,
                    reason,
                    button: self.github_refusal.button.clone(),
                    controls: self.github_refusal.controls.clone(),
                    last_controls: self.github_refusal.last_controls.clone(),
                    side: self.github_refusal.side.clone(),
                    placed: self.github_refusal.placed.clone(),
                    drawn: kept,
                })
                .with_priority(TOOLTIP_PRIORITY)
                .into_any_element(),
            );
        }
        drawn
    }

    /// Forgets where the header's and the tab strip's controls lay in the frame before, as the
    /// tab strip, the first of their rows, is built for a frame ([`OpenRefusal::controls`]).
    pub(super) fn begin_header_controls(&self) {
        let mut controls = self.github_refusal.controls.borrow_mut();
        let mut last = self.github_refusal.last_controls.borrow_mut();
        std::mem::swap(&mut *last, &mut *controls);
        controls.clear();
    }

    /// `control`, one of the header's or the tab strip's, recording where it is prepainted in
    /// this frame, where its clip leaves it shown, which the reason's popup at Pull requests
    /// stands clear of ([`OpenRefusal::controls`]). For an element whose own children decide
    /// nothing of its size; [`Self::framed_header_control`] for any other.
    pub(super) fn header_control<E: ParentElement>(&self, control: E) -> E {
        let controls = self.github_refusal.controls.clone();
        gpui_kit::base::ElementExt::on_prepaint(control, move |bounds, window, _| {
            let shown = bounds.intersect(&window.content_mask().bounds);
            if shown.size.width > px(0.) && shown.size.height > px(0.) {
                controls.borrow_mut().push(shown);
            }
        })
    }

    /// [`Self::header_control`] for a control a child of its own would change, such as a
    /// Button with no label, which the kit sizes as a square only while it holds no child, or
    /// one that takes no child: a frame around it, as wide and as tall as it, records it.
    pub(super) fn framed_header_control(&self, control: impl IntoElement) -> Div {
        self.header_control(div().flex().flex_shrink_0().child(control))
    }

    /// Open the panel from the header's Pull requests button, full or
    /// compact, which closing returns focus to.
    /// A press by Return or Space that went down while the panel could open and comes up
    /// refused is answered at the button too, as [`Self::pull_requests_key_down`] answers one
    /// refused as it goes down.
    pub(super) fn open_github_from_header(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page == AppPage::Repository
            && window.last_input_was_keyboard()
            && let Some(reason) = self.github_open_refusal()
        {
            self.answer_pull_requests_at_button(reason, window, cx);
            return;
        }
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
        if self.page != AppPage::Repository {
            return;
        }
        // Answered now or not at all: a press that cannot open the panel says why and leaves
        // everything as it was, and no open waits to happen once the person has moved on.
        if let Some(reason) = self.github_open_refusal() {
            self.say_github_open_refused(reason, cx);
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
        let reopened = cached.is_some();
        let form = if let Some(form) = cached {
            let window = window.window_handle().window_id();
            form.update(cx, |panel, cx| {
                panel.closed = false;
                panel.hear_keys(window, cx);
                panel.confirm = None;
                // Reopening onto its destination field, its first part, the panel shows its
                // body from the top, the banner above the field included. Closed with the body
                // scrolled down, it reopened there, the field at the body's edge and what lies
                // above it out of view, since the field alone was whole. Reopening onto
                // another control of the body that held focus as it closed, as an open PR or
                // a new one does, keeps the body where it was, and the first frame reveals the
                // control where a size change while closed moved it ([`review::Reopened`]).
                if panel.reopens_onto_destination(cx) {
                    panel.review.body_scroll.set_offset(point(px(0.), px(0.)));
                }
                cx.notify();
            });
            form
        } else {
            cx.new(|cx| Panel::new(owner, repo, branch, window, cx))
        };
        // The panel's controls with keys of their own, which the status strip names while
        // they hold focus. Its Buttons and fields have none, so they register nothing.
        let keyed = {
            let panel = form.read(cx);
            [
                (
                    panel.review.file_focus.clone(),
                    shortcuts::DialogKeys::GithubFiles,
                ),
                (
                    panel.review.line_focus.clone(),
                    shortcuts::DialogKeys::GithubPatch,
                ),
                (
                    panel.conversations.confirm_focus.clone(),
                    shortcuts::DialogKeys::GithubConfirmation,
                ),
            ]
        };
        for (focus, keys) in &keyed {
            self.dialog_hints.register(focus, *keys);
        }
        let focus_form = form.clone();
        let ring = self.focus_ring.clone();
        window.defer(cx, move |window, cx| {
            focus_form.update(cx, |panel, cx| {
                panel.load_local(window, cx);
                // Closing cancelled a pending preparation; resume it in whichever mode the
                // file was left, keeping its scroll and Source editor.
                if let Some(index) = panel.review.take_suspended() {
                    panel.prepare_review_file(index, window, cx);
                }
                // Reopened, focus goes back where it was only where it is still in view.
                if reopened {
                    panel.focus_reopened(window, cx);
                } else {
                    panel.focus_visible_section(window, cx);
                }
            });
            // Opened or reopened, by a click as by a key, the focus the panel lands on draws
            // its ring, as Settings opens, until the next pointer press: reopened by a click on
            // the header's Pull requests, it gave Files' patch back with no ring.
            ring.mark(window);
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
                        // Back to repository stands where the panel hears Return and Space go
                        // down and come up, as its body does ([`HeldKeys`]).
                        div()
                            .key_context(HELD)
                            .capture_key_up(window.listener_for(&form, Panel::note_key_up))
                            .child(
                                button("github-close", "Back to repository", "", false)
                                    .debug_selector(|| "github-close".into())
                                    .on_click(move |_, window, cx| {
                                        close.update(cx, |this, cx| this.close(window, cx))
                                    }),
                            ),
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
/// The pull request the loaded list holds for a creation GitHub refused because its head
/// branch already has one ([`Panel::refused_for_existing`]): what the failure line's one
/// action, Open #N, opens, as a press on its row does. `line` is the failure line it was found
/// for, and the action is drawn only beside that line, so it never stands by another failure;
/// `repository` is the one the creation was refused in, whose pull request `pull` is, and the
/// action is offered only while the destination names it ([`Panel::offered`]): a number alone
/// names another pull request in every other repository.
struct Existing {
    line: String,
    repository: Repository,
    pull: PullRequest,
}
/// The row among `rows`, the loaded page of `new`'s repository, that is the open pull request
/// a creation of `new` was refused for: the one open row from the same head, the same owner
/// and the same branch, as GitHub names a head (`owner:branch`), onto the same base branch. A
/// same-named branch of another fork is another head, and a row whose head names no owner, or
/// a second row that fits, tells nothing for certain: then there is none, since naming another
/// pull request is worse than naming none. An owner's name matches whatever its case, as
/// GitHub's logins do.
fn held_pull<'a>(rows: &'a [PullRequest], new: &NewPull) -> Option<&'a PullRequest> {
    let (owner, branch) = new
        .head
        .split_once(':')
        .unwrap_or((new.repository.owner.as_str(), new.head.as_str()));
    let mut held = rows.iter().filter(|row| {
        row.state == "open"
            && row.base.branch == new.base
            && row.head.branch == branch
            && row
                .head
                .label
                .split_once(':')
                .is_some_and(|(by, named)| by.eq_ignore_ascii_case(owner) && named == branch)
    });
    match (held.next(), held.next()) {
        (Some(row), None) => Some(row),
        _ => None,
    }
}
/// The body's content in the frame that first draws a creation's result
/// ([`Panel::show_created`]), which shows the result from its notice: where this frame's
/// layout puts the notice's line (`line`) and the list below it (`list`), whose first row is
/// the new one, not both whole in `body`'s view, the body scrolls that line, the start of a
/// block, to its top, as an opening pull request shows from its title, so the notice and the
/// row under it are in view together. It does so before the content prepaints, so this frame
/// draws both where they stay and no later frame moves them: the first frame after a change
/// lays out its final state (DESIGN.md, Motion). The body once followed the row's own reveal
/// first, which follows focus and shows the row by the least movement, and came to the
/// notice four frames of the list later: at 18 pt in a 461 × 490 window the notice stood
/// 33 px above the body's top until then, and jumped into view about 50 ms after the row.
/// Made for that frame alone; every other frame's content is drawn as it is.
struct Created {
    content: AnyElement,
    body: ScrollHandle,
    line: std::rc::Rc<std::cell::Cell<Option<LayoutId>>>,
    list: std::rc::Rc<std::cell::Cell<Option<LayoutId>>>,
}
impl Created {
    /// How far the body moves its content to show the notice and the list together, having
    /// moved its scroll by as much, if it moves. The body has placed its content at its offset
    /// already, so each place read here is where this frame would draw it.
    fn show(&self, window: &mut Window) -> Option<Pixels> {
        let line = window.layout_bounds(self.line.get()?);
        let list = window.layout_bounds(self.list.get()?);
        let (view, offset) = (self.body.bounds(), self.body.offset());
        if line.top() >= view.top() && list.bottom() <= view.bottom() {
            return None;
        }
        let y = (offset.y + view.top() - line.top()).clamp(-self.body.max_offset().y, px(0.));
        let moved = y - offset.y;
        if moved == Pixels::ZERO {
            return None;
        }
        self.body.set_offset(point(offset.x, y));
        Some(moved)
    }
}
impl IntoElement for Created {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for Created {
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
        (self.content.request_layout(window, cx), ())
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
        // Content the body moves is prepainted where its new offset puts it.
        match self.show(window) {
            Some(moved) => window.with_element_offset(point(Pixels::ZERO, moved), |window| {
                self.content.prepaint(window, cx);
            }),
            None => {
                self.content.prepaint(window, cx);
            }
        }
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
        self.content.paint(window, cx);
    }
}
/// How a request of the panel opens its transport once the operation executor runs it
/// ([`Panel::opener`]): the offline fixture's or the connected account's
/// ([`UiTransport::open`]), or, in a test that gave the panel one, a script's answers.
struct Opener {
    fixture: bool,
    moved: bool,
    #[cfg(test)]
    scripted: Option<github::Scripted>,
}
impl Opener {
    fn open(self) -> anyhow::Result<PanelTransport> {
        #[cfg(test)]
        if let Some(scripted) = self.scripted {
            return Ok(PanelTransport::Scripted(scripted));
        }
        Ok(PanelTransport::Ui(UiTransport::open(
            self.fixture,
            self.moved,
        )?))
    }
}
/// The transport an [`Opener`] opened.
enum PanelTransport {
    Ui(UiTransport),
    #[cfg(test)]
    Scripted(github::Scripted),
}
impl PanelTransport {
    /// The account its requests are sent as.
    fn login(&self) -> &str {
        match self {
            Self::Ui(transport) => transport.login(),
            #[cfg(test)]
            Self::Scripted(_) => github::Scripted::LOGIN,
        }
    }
}
impl github::Transport for PanelTransport {
    fn request(
        &mut self,
        request: github::Request,
        control: &OperationControl,
    ) -> anyhow::Result<github::Response> {
        match self {
            Self::Ui(transport) => transport.request(request, control),
            #[cfg(test)]
            Self::Scripted(transport) => transport.request(request, control),
        }
    }
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
/// Return's and Space's presses going down in the panel, so that a control a landing gives
/// focus takes no press that was down as it landed ([`Panel::land_focus`]): the new pull
/// request's row after a creation, and Open #N or Refresh PRs after one refused for an existing
/// pull request; and so that the text field Send to GitHub hands focus to as a send starts,
/// a creation's title, the review body or a reply, takes no text from one
/// ([`Panel::hear_keys`]). GPUI clicks the focused control as Space or Return comes up where it went
/// down at the same focus, and its X11 client hands a held key's repeat on as a release and a
/// fresh press where the two reach it in separate batches, so a Space still held on Send to
/// GitHub as its creation landed opened the new pull request at the repeat after the one that
/// found focus on its row, and a Return whose release ended a batch did at once: a read from
/// GitHub nobody pressed. Each key keeps its own press, as the Diff view's do
/// (`diff_view::Presses`), from its first key-down to the first frame after its key-up
/// ([`Panel::note_key_up`]). The panel hears a key go down through a keystroke interceptor
/// ([`Panel::new`]) and not a key listener: a key bound in the focused control's context, as
/// Return is in a field, reaches no listener, and a Return held on Send repeats in the form's
/// first field while the creation is sent. It counts only where the panel hears the key come
/// up too, with focus inside the body, on the line above it or on the dialog footer's Back to
/// repository ([`HELD`]): everywhere the panel's dialog draws a stop.
///
/// A press never outlasts the hearing of its key. The panel's close ends every press
/// ([`Panel::close`]), and a press under way as focus goes to nothing the window draws, where
/// no key-up is heard, is marked as one whose key-up may have been lost ([`HeldKeys::leave`]).
/// The footer once stood outside the hearing: a Return that sent and came up on Back to
/// repository, like a key down as the panel closed, stayed recorded, the next landing latched
/// it with no key down, and the other key did nothing on the controls a landing focuses until
/// the recorded one had been pressed once; and a Space held on Back to repository across a
/// landing, which the panel never heard go down, opened the new pull request at its next
/// repeat.
#[derive(Default)]
struct HeldKeys {
    /// Return's press, then Space's ([`held_key`]).
    keys: [Option<HeldKey>; 2],
    /// Escape's press, heard as theirs are, from its key-down to the first frame after its
    /// key-up. Only the end of a send that refused it latches it ([`HeldKeys::latch_escape`]),
    /// and no other key's press reads it: an Escape answered "wait for the result" and still
    /// down as the result comes closes nothing until it has come up.
    escape: Option<HeldKey>,
    /// The number of the last key-up that released one.
    key_ups: u64,
    /// While a press is latched, marks the presses as focus leaves the control the landing
    /// gave it, for another control, with the panel as it closes, or with the window as it
    /// turns inactive ([`HeldKeys::leave`]).
    watch: Option<Subscription>,
}
impl HeldKeys {
    /// Whether `event` is a keyboard's click while a press that was down as a landing gave a
    /// control focus is still under way: such a click does nothing. A pointer's click is not
    /// the held key's, and acts.
    fn refuses(&self, event: &ClickEvent) -> bool {
        matches!(event, ClickEvent::Keyboard(_)) && self.latched()
    }
    /// Whether a latched press is under way.
    fn latched(&self) -> bool {
        self.keys.iter().flatten().any(|press| press.latched)
    }
    /// Marks each press under way as focus leaves where its key-up would be seen: it may come
    /// up where the panel never sees it ([`HeldKey::left`]). Nothing ends here: under X11
    /// another client's keyboard grab takes the window's focus and gives it back while the
    /// key is still down, and its next repeat would then press what the landing focused.
    fn leave(&mut self) {
        for press in self.keys.iter_mut().flatten() {
            press.left = true;
        }
    }
    /// Records Return, else Space, going down in the panel. A key-down of a key whose press
    /// is under way repeats it, marked held or not, also right after its key-up, as an X11
    /// repeat's release and press arrive, and after focus left, as a key still held repeats
    /// once focus is back. Otherwise the key's press begins: latched if the other key's is,
    /// as a key pressed while that one is down acts no more than it does, and ending the
    /// other key's press where focus left while it was down ([`HeldKey::left`]), so a key-up
    /// lost outside costs this one press and no more.
    ///
    /// A keystroke the platform sends no key-up for (`withheld`, [`key_up_withheld`]) begins
    /// no press and repeats none: nothing would end a press it began. A Command-Return on
    /// Send to GitHub, which sends nothing, once began a press that never ended on macOS; the
    /// next send's hand-off latched it ([`Panel::land_focus`]), and every Space typed in the
    /// review body after that send was stopped until Return had been pressed again.
    fn key_down(&mut self, own: usize, withheld: bool) {
        if withheld {
            return;
        }
        if let Some(press) = self.keys[own].as_mut() {
            press.released = None;
            press.left = false;
            return;
        }
        let other = self.keys[1 - own];
        self.keys[own] = Some(HeldKey {
            latched: other.is_some_and(|press| press.latched),
            released: None,
            left: false,
        });
        if other.is_some_and(|press| press.left) {
            self.keys[1 - own] = None;
        }
    }
    /// Records Escape going down in the panel: its press begins, or repeats where one is under
    /// way, marked held or not, also right after its key-up, as an X11 repeat's release and
    /// press arrive. Returns whether the press is latched ([`Self::latch_escape`]), its
    /// key-down then one to stop.
    fn escape_down(&mut self) -> bool {
        let press = self.escape.get_or_insert(HeldKey {
            latched: false,
            released: None,
            left: false,
        });
        press.released = None;
        press.latched
    }
    /// A send that refused Escape has ended ([`Panel::run_from`]): an Escape press still under
    /// way is latched, so that its key-downs are stopped until it has come up
    /// ([`Panel::hear_keys`]). The refusal said to wait for the result; the next repeat of the
    /// same press, under X11 a fresh key-down, once found the send ended and closed the panel
    /// on the result it had just shown. A key-up lost meanwhile costs one Escape at most: the
    /// next one is taken for the held key's and ends the hold as it comes up.
    fn latch_escape(&mut self) {
        if let Some(press) = self.escape.as_mut() {
            press.latched = true;
        }
    }
    /// The press of the key kept at `key`: Return's or Space's ([`held_key`]), or Escape's
    /// ([`ESCAPE`]).
    fn press(&mut self, key: usize) -> &mut Option<HeldKey> {
        match key {
            ESCAPE => &mut self.escape,
            own => &mut self.keys[own],
        }
    }
}
/// The key context of the panel's top line and body together while the line stands above the
/// body, where its Open #N is no part of the body ([`review::BODY`]), and of the dialog
/// footer's Back to repository: with focus in any of them, the panel hears Return and Space
/// come up ([`HeldKeys`]).
const HELD: &str = "GitTurtleGithubHeldKeys";
/// The key context the toolkit gives a text field, one line or many (`gpui_kit::base::input`):
/// with focus in one of the panel's, a latched press types nothing ([`Panel::hear_keys`]).
const FIELD: &str = "Input";
/// A Return or Space press in the panel ([`HeldKeys`]).
#[derive(Clone, Copy)]
struct HeldKey {
    /// Whether it was down as a landing gave a control focus, or went down while such a press
    /// of the other key was: until it ends, no keyboard click acts on the controls a landing
    /// gives focus ([`HeldKeys::refuses`]).
    latched: bool,
    /// The number of the key-up that released it, until it ends at the next frame: a key-down
    /// before then repeats it, as the press of an X11 repeat follows its release.
    released: Option<u64>,
    /// Whether focus left while it was down, so its key-up may have been lost: the next
    /// key-down of its key repeats it all the same, as a key still held would, and a press the
    /// other key begins ends it and inherits its latch, so a lost key-up costs one press at
    /// most and never leaves the keyboard refused.
    left: bool,
}
/// Where [`HeldKeys`] keeps `keystroke`'s press, Return's then Space's, whatever modifiers it
/// carries: under X11 a repeat carries those held as it repeats. No other key has one.
fn held_key(keystroke: &gpui_kit::Keystroke) -> Option<usize> {
    match keystroke.key.as_str() {
        "enter" => Some(0),
        "space" => Some(1),
        _ => None,
    }
}
/// Escape's place among the presses [`HeldKeys`] keeps ([`HeldKeys::press`]).
const ESCAPE: usize = 2;
/// Where [`HeldKeys`] keeps the press `keystroke` is of: Return's or Space's ([`held_key`]), or
/// Escape's ([`ESCAPE`]), whatever modifiers it carries.
fn heard_key(keystroke: &gpui_kit::Keystroke) -> Option<usize> {
    held_key(keystroke).or_else(|| (keystroke.key == "escape").then_some(ESCAPE))
}
/// Whether the platform sends no key-up for `keystroke`, as Cocoa sends none for a key
/// pressed with Command held on macOS. A key pressed with any other modifier sends one, and
/// off macOS so does a key pressed with Super, the platform modifier there. The toolkit's
/// record of a held Return goes by the same condition (`gpui-base`'s `dialog.rs`,
/// `key_up_withheld`); unverified without a Mac, as that one is.
fn key_up_withheld(keystroke: &gpui_kit::Keystroke) -> bool {
    cfg!(target_os = "macos") && keystroke.modifiers.platform
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
#[cfg(test)]
thread_local! {
    /// What every request's busy line says instead of its own, for a test of a line longer
    /// than any of them.
    static BUSY_LABEL_FOR_TEST: std::cell::Cell<Option<&'static str>> =
        const { std::cell::Cell::new(None) };
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
    pub(crate) fn label(self) -> &'static str {
        #[cfg(test)]
        if let Some(label) = BUSY_LABEL_FOR_TEST.with(std::cell::Cell::get) {
            return label;
        }
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
    /// Whether it only reads, so that once cancelled it can drain behind a reopened panel
    /// ([`GitTurtle::github_open_refusal`]). Connect and Disconnect change the stored
    /// account, and Send what GitHub holds.
    fn reads_only(self) -> bool {
        matches!(self, Self::Load | Self::Compare | Self::Templates)
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
    gitturtle_core::is_read_cancelled(error) || error.downcast_ref::<NotStarted>().is_some()
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
    use crate::github::transport::{GhTransport, SecureStore};
    use gitturtle_core::OperationControl;
    #[cfg(target_os = "linux")]
    use std::time::Duration;

    #[cfg(target_os = "linux")]
    pub(super) struct ParentWindowHandle {
        future: futures::future::BoxFuture<'static, Option<ashpd::WindowIdentifier>>,
        executor: gpui_kit::BackgroundExecutor,
    }

    #[cfg(not(target_os = "linux"))]
    pub(super) struct ParentWindowHandle;

    pub(super) fn capture_parent_window(cx: &gpui_kit::App) -> ParentWindowHandle {
        #[cfg(target_os = "linux")]
        {
            ParentWindowHandle {
                future: gpui_linux::active_window_identifier(),
                executor: cx.background_executor().clone(),
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = cx;
            ParentWindowHandle
        }
    }

    #[cfg(target_os = "linux")]
    fn resolve_parent(
        handle: ParentWindowHandle,
    ) -> (Option<ashpd::WindowIdentifier>, Option<String>) {
        use futures::future::{Either, select};
        use std::pin::pin;
        let guard = futures::executor::block_on(async move {
            let fut = pin!(handle.future);
            let timeout = pin!(handle.executor.timer(Duration::from_millis(250)));
            match select(fut, timeout).await {
                Either::Left((id, _)) => id,
                Either::Right(_) => None,
            }
        });
        let raw = guard.as_ref().map(ToString::to_string);
        (guard, raw)
    }

    pub(super) fn connect(
        control: &OperationControl,
        parent: ParentWindowHandle,
    ) -> anyhow::Result<String> {
        #[cfg(target_os = "linux")]
        {
            // Keep `_guard` alive until Secret Service prompts complete so Wayland
            // `xdg-foreign` exported surface handles remain valid.
            let (_guard, window_id) = resolve_parent(parent);
            GhTransport::connect_existing_with_window_id(control, window_id)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = parent;
            GhTransport::connect_existing(control)
        }
    }
    /// `control` is the request's own, so its Cancel stops a keyring prompt on Linux.
    pub(super) fn remove(
        control: &OperationControl,
        parent: ParentWindowHandle,
    ) -> anyhow::Result<()> {
        #[cfg(target_os = "linux")]
        {
            let (_guard, window_id) = resolve_parent(parent);
            SecureStore.remove_with_window_id(control, window_id)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = parent;
            SecureStore.remove_with_window_id(control, None)
        }
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
    pub(super) struct ParentWindowHandle;
    pub(super) fn capture_parent_window(_cx: &gpui_kit::App) -> ParentWindowHandle {
        ParentWindowHandle
    }
    /// Every control a removal got, so a test can tell it was the request's own, whose
    /// Cancel stops the keyring's prompt, each with the settings directory of the test whose
    /// operation executor ran it, which an executor inherits from the test that started it
    /// ([`crate::preferences::test_settings_directory`]), so a test reads its own removals
    /// only ([`removed_with`]). Every test in the process shares the record: read whole, a
    /// test running beside another that disconnects could take the other's control for its
    /// own.
    static REMOVED_WITH: Mutex<Vec<(std::path::PathBuf, OperationControl)>> =
        Mutex::new(Vec::new());
    pub(super) fn connect(_: &OperationControl, _: ParentWindowHandle) -> anyhow::Result<String> {
        Ok(LOGIN.into())
    }
    pub(super) fn remove(control: &OperationControl, _: ParentWindowHandle) -> anyhow::Result<()> {
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
/// What Escape, or Back to repository, is answered with while a send is in flight
/// ([`Panel::close`]): to wait for that send's result, so the line goes as the result comes
/// ([`Panel::run_from`]).
const SEND_IN_FLIGHT: &str = "A GitHub action is in progress for this captured destination. Wait for the result, or choose Cancel and inspect GitHub before resubmitting.";
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
/// The key a new pull request's draft is saved under ([`Draft::key`]): its destination, its
/// head and its base. The New pull request form's text belongs to one, the key it was typed
/// or restored under ([`Panel::settle_form`]).
#[derive(Clone, Debug, PartialEq, Eq)]
struct FormKey {
    repository: Repository,
    head: String,
    base: String,
}
impl FormKey {
    /// The key `form`'s fields name.
    fn of(form: &NewPull) -> Self {
        Self {
            repository: form.repository.clone(),
            head: form.head.clone(),
            base: form.base.clone(),
        }
    }
    /// Whether `pull` is a draft of this key.
    fn names(&self, pull: &NewPull) -> bool {
        pull.repository == self.repository && pull.head == self.head && pull.base == self.base
    }
    /// The draft that keeps `form`'s title, description and Draft or Ready under this key,
    /// whatever key the form's fields name.
    fn keep(&self, form: &NewPull) -> NewPull {
        NewPull {
            repository: self.repository.clone(),
            head: self.head.clone(),
            base: self.base.clone(),
            ..form.clone()
        }
    }
}
/// The New pull request form's text, whole: its title, its description and Draft or Ready.
#[derive(Clone, Debug, PartialEq, Eq)]
struct FormText {
    title: String,
    body: String,
    draft: bool,
}
/// Whether two creation drafts hold the same text: title, description and Draft or Ready.
fn same_text(one: &NewPull, other: &NewPull) -> bool {
    one.title == other.title && one.body == other.body && one.draft == other.draft
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
/// What the New pull request form's save status reads once it shows a saved draft it took
/// whole, until the next edit ([`Panel::settle_form`]): the words a restored review's has.
const DRAFT_RESTORED: &str = "Draft restored locally";
/// What the form's save status reads while its text has no key and another draft holds the
/// one its fields name ([`Panel::settle_form`]): neither is put in the other's place. It
/// names the three things that key is made of, and what it says of the window holds
/// ([`Panel::leave_form`]).
const DRAFT_HELD: &str = "Draft not saved: another draft is saved for this destination, head and base. Discard it in Recover drafts to save this one. Keep this window open or copy your text.";
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
        Ok(Draft::Review(review)) if drafts::is_empty_review(&review) => "No local draft yet",
        Ok(_) if !succeeded => "Draft save failed — keep this window open or copy the text",
        Ok(_) if pending => "Saving local draft…",
        Ok(_) => "Draft saved locally",
    }
}

fn is_recoverable_draft(draft: &Draft) -> bool {
    !matches!(draft, Draft::Reply(reply) if reply.body.is_empty())
        && !matches!(draft, Draft::Review(review) if drafts::is_empty_review(review))
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
    /// The repository whose open pull requests `rows` lists, once Refresh PRs or a page of
    /// them loaded: a refused creation names a pull request only from its own repository's
    /// rows ([`Panel::refused_for_existing`]).
    listed: Option<Repository>,
    /// The pull request the failure line offers to open ([`Existing`]).
    existing: Option<Existing>,
    /// Whether `rows` holds only pull requests a creation's answer gave, with no page GitHub
    /// listed: the list then says so, since it must not read as the open pull requests, and
    /// Refresh PRs loads those ([`Panel::show_created`]).
    created_only: bool,
    /// Set as a creation's row joins the list: the frame that next draws the list shows the
    /// notice that names the pull request and its row together ([`Created`]), and the row's
    /// own reveal, which follows focus, counts as made.
    show_row: std::rc::Rc<std::cell::Cell<bool>>,
    /// Set as a failure lands that the body must show wherever it has scrolled: the frame
    /// that next draws the top line among the content shows the line in that frame
    /// ([`Failed`]).
    show_line: std::rc::Rc<std::cell::Cell<bool>>,
    /// The part each of `rows`' titles is in the body (`pr-{number}`), built once for each page
    /// the rows change to ([`Panel::name_row_parts`]) rather than for each row in each frame.
    row_parts: Vec<(u64, SharedString)>,
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
    description_view: Option<Entity<crate::markdown_view::View>>,
    description_source: bool,
    pr_filter_open: bool,
    pr_query: github::PullListQuery,
    pr_search: Entity<InputState>,
    checks_expanded: bool,
    create: bool,
    /// Whether the description's field holds a review's text, restored or typed for an opened
    /// pull request, rather than what the New pull request form left in it: the form and the
    /// review body share the field, and the form opens by clearing it only of a review's text
    /// ([`Panel::create`]).
    reviewed: bool,
    /// The key the form's text belongs to ([`FormKey`]): the one it was typed or restored
    /// under, which the form is saved under and no other, whatever its fields name meanwhile
    /// ([`Panel::settle_form`]). None while the text has no key, typed with a destination
    /// that names no repository, or while another draft holds the key its fields name.
    home: Option<FormKey>,
    /// Whether an edit of the destination, the head or the base has named a key that does
    /// not count yet: the field's loss of focus settles it ([`Panel::settle_key`]). Focus
    /// that only passes through one of the three settles nothing.
    naming: bool,
    /// Whether the form shows a saved draft it took whole, with nothing edited since: its
    /// status says so until the next edit ([`DRAFT_RESTORED`]).
    restored: bool,
    /// What the New pull request form held as it was left, whole, while the panel shows
    /// anything else ([`Panel::leave_form`]): the form opens again on it
    /// ([`Panel::create`]). The store may hold none of it, and the description's field is
    /// the review body's meanwhile.
    left: Option<FormText>,
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
    /// The request Escape was refused during, by its generation ([`Panel::close`]): the
    /// refusal says to wait for that request's result, so its line goes as the request ends,
    /// before the result is received ([`Panel::run_from`]).
    refused_during: Option<u64>,
    /// Where `error` or `cancelled` answers: the control the request or refusal they answer
    /// started from ([`Origin`]).
    origin: Origin,
    /// The presses answered with `error` and `draft_issue` ([`Presses`]).
    presses: Presses,
    /// Return's and Space's presses going down in the panel ([`HeldKeys`]), shared with the
    /// keystroke interceptor that hears them go down, kept while the panel is open
    /// ([`Self::hear_keys`]).
    held: std::rc::Rc<std::cell::RefCell<HeldKeys>>,
    hears_keys: Option<Subscription>,
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
    saver: commit_drafts::CoalescingSaver<String, drafts::Change>,
    deferred_draft: DeferredDraft,
    draft_timer: Option<Task<()>>,
    save_response_generation: u64,
    save_completion: Option<DraftSaveCompletion>,
    subscriptions: Vec<Subscription>,
    templates: Vec<github::Template>,
    attempts: Vec<drafts::Attempt>,
    local_control: Option<HistoryCancellation>,
    /// Linux: whether a passwordless `Default_keyring.keyring` was detected on disk
    /// (and not yet dismissed by the user). Always false in test builds unless injected.
    #[cfg(target_os = "linux")]
    plaintext_keyring_note: bool,
    /// The script a test's requests are answered from, in place of a transport ([`Opener`]).
    #[cfg(test)]
    scripted: Option<github::Scripted>,
}

/// Calm, one-time informational note displayed on Linux when the user's desktop
/// keyring uses a passwordless `Default_keyring.keyring` (stock Omarchy default).
#[cfg(target_os = "linux")]
const LINUX_PLAINTEXT_KEYRING_NOTE: &str = "Your desktop keyring is currently unlocked without a login password (Default_keyring.keyring), so stored tokens remain readable on disk by processes running as your user. You can set a keyring password in your desktop keyring settings.";

/// Returns true when `path` is a regular non-symlink file beginning with `[keyring]`,
/// the header gnome-keyring writes for passwordless keyrings.
#[cfg(target_os = "linux")]
fn is_passwordless_gnome_keyring_file(path: &std::path::Path) -> bool {
    use rustix::fs::{Mode, OFlags, open};
    use std::io::Read as _;
    let Ok(fd) = open(
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    ) else {
        return false;
    };
    let mut file = std::fs::File::from(fd);
    let Ok(meta) = file.metadata() else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    let mut buf = [0u8; 64];
    let Ok(n) = file.read(&mut buf) else {
        return false;
    };
    let head = std::str::from_utf8(&buf[..n]).unwrap_or("");
    head.trim_start().starts_with("[keyring]")
}

#[cfg(target_os = "linux")]
fn detect_passwordless_default_keyring() -> bool {
    #[cfg(test)]
    {
        false
    }
    #[cfg(not(test))]
    {
        let data_dir = std::env::var_os("XDG_DATA_HOME")
            .filter(|value| !value.is_empty())
            .map(std::path::PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME")
                    .filter(|value| !value.is_empty())
                    .map(std::path::PathBuf::from)
                    .filter(|path| path.is_absolute())
                    .map(|home| home.join(".local/share"))
            });
        data_dir.is_some_and(|dir| {
            is_passwordless_gnome_keyring_file(&dir.join("keyrings/Default_keyring.keyring"))
        })
    }
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
        #[cfg(target_os = "linux")]
        cx.spawn(async move |panel, cx| {
            // Defer reading `owner` until after `open_github_returning_to`
            // releases its mutable lease on `GitTurtle`.
            let dismissed = panel
                .read_with(cx, |this, cx| {
                    this.owner.upgrade().is_some_and(|app| {
                        app.read(cx).settings.linux_plaintext_keyring_note_dismissed
                    })
                })
                .unwrap_or(true);
            if dismissed {
                return;
            }
            let detected = cx
                .background_executor()
                .spawn(async { detect_passwordless_default_keyring() })
                .await;
            if detected {
                let _ = panel.update(cx, |this, cx| {
                    let still_undismissed = this.owner.upgrade().is_none_or(|app| {
                        !app.read(cx).settings.linux_plaintext_keyring_note_dismissed
                    });
                    if still_undismissed {
                        this.plaintext_keyring_note = true;
                        cx.notify();
                    }
                });
            }
        })
        .detach();
        let mut this=Self {
            review:review::ReviewState::new(window,cx),
            conversations:conversations::State::new(window,cx),
            owner,worktree,repo,destination:cx.new(|cx|InputState::new(window,cx).placeholder("owner/repository")),
            head:cx.new(|cx|InputState::new(window,cx).default_value(branch).placeholder("Published head branch (owner:branch for a fork)")),
            base:cx.new(|cx|InputState::new(window,cx).default_value("main")),title:cx.new(|cx|InputState::new(window,cx).placeholder("Pull request title")),
            body:cx.new(|cx|TextareaState::new(window,cx).auto_grow(5,5).placeholder("Description or review text — saved locally")),
            rows:vec![],listed:None,existing:None,created_only:false,show_row:Default::default(),show_line:Default::default(),row_parts:Vec::new(),page:1,has_next:false,pull:None,pressed:Default::default(),status:None,description:None,description_view:None,description_source:false,pr_filter_open:false,pr_query:github::PullListQuery::default(),pr_search:cx.new(|cx|InputState::new(window,cx).placeholder("Filter by title, branch, or author")),checks_expanded:false,create:false,draft:true,event:ReviewEvent::Comment,discussion:true,
            account:None,notice:Some("Offline until you choose Connect or Refresh. GitHub.com is supported; Git authentication and Push keep their existing behavior.".into()),heard:Heard::default(),top_line_said:Heard::default(),error:None,cancelled:None,refused_during:None,origin:Origin::Panel,presses:Presses::default(),held:Default::default(),hears_keys:None,pending:false,cancel_request:focus_ring::LoadingCancel::new(cx),request_generation:0,local_loaded:false,local_loading:false,local_generation:0,local_task:None,request:RequestKind::Load,closed:false,
            confirm:None,control:None,task:None,saved:vec![],save_status:SharedString::default(),draft_issue:None,saver:Default::default(),deferred_draft:Default::default(),draft_timer:None,save_response_generation:0,save_completion:None,subscriptions:vec![],templates:vec![],attempts:vec![],local_control:None,
            reviewed: false,
            home: None,
            naming: false,
            restored: false,
            left: None,
            #[cfg(target_os = "linux")]
            plaintext_keyring_note: false,
            #[cfg(test)]
            scripted:None,
        };
        if this.review.fixture {
            this.destination.update(cx, |input, cx| {
                input.set_value("gitturtle-fixture/native-review", window, cx)
            });
            this.account = Some("offline-reviewer".into());
            this.notice=Some("Offline review fixture · fixed sample data; account connection and network transport are unavailable in this mode.".into());
        }
        // A pane that goes with focus hands it on ([`Panel::hand_off_lost_pane_focus`]).
        this.subscriptions
            .push(cx.on_focus_lost(window, Self::hand_off_lost_pane_focus));
        // A key held as the window turns inactive may come up outside it ([`HeldKeys::leave`]).
        this.subscriptions
            .push(cx.observe_window_activation(window, |this, window, _| {
                if !window.is_window_active() {
                    this.held.borrow_mut().leave();
                }
            }));
        // As may one held as focus goes to nothing the window draws, where no key-up is heard.
        this.subscriptions
            .push(cx.on_focus_lost(window, |this, _, _| this.held.borrow_mut().leave()));
        this.hear_keys(window.window_handle().window_id(), cx);
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
        let pr_search = this.pr_search.clone();
        this.subscriptions.push(cx.subscribe_in(
            &pr_search,
            window,
            |this, input, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.pr_query.search = input.read(cx).value().to_string();
                    cx.notify();
                }
            },
        ));
        for input in [this.head.clone(), this.base.clone(), this.title.clone()] {
            this.subscriptions.push(cx.subscribe_in(
                &input,
                window,
                |this, input, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => {
                        this.confirm = None;
                        // An edit of the head or the base names another draft's key, which
                        // counts once the field is left ([`Self::settle_form`]).
                        if input == &this.title {
                            this.defer_draft_save(window, cx);
                        } else {
                            this.key_edited(window, cx);
                        }
                    }
                    InputEvent::Blur if input != &this.title && this.naming => {
                        this.settle_key(window, cx)
                    }
                    _ => {}
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
                if matches!(event, InputEvent::Blur) && this.naming {
                    this.settle_key(window, cx);
                }
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
                    // The rows went, so the list holds no repository's page, nor rows a
                    // creation gave; and the pull request a refused creation named was the
                    // repository's the field named until now, so its line and action go too.
                    this.listed = None;
                    this.created_only = false;
                    if let Some(existing) = this.existing.take()
                        && this.error.as_ref() == Some(&existing.line)
                    {
                        this.error = None;
                    }
                    this.pull = None;
                    this.status = None;
                    this.description = None;
                    this.confirm = None;
                    this.page = 1;
                    this.has_next = false;
                    // As an edit of the head does, this one names another draft's key.
                    this.key_edited(window, cx);
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
                                // Whether the form saved its text under its key before
                                // this read landed, in place of whatever the store held.
                                let own = this.stored_at_home().is_some();
                                for draft in std::mem::take(&mut this.saved) {
                                    let key = draft.key();
                                    saved.retain(|old| old.key() != key);
                                    saved.push(draft);
                                }
                                this.saved = saved;
                                // The key counts as the saved drafts are read: a draft the
                                // read found under it, which the form neither showed nor
                                // saved over, is no text of the form's, so the form's has
                                // no key until the rule gives it one ([`Self::settle_form`]).
                                if !own && this.stored_at_home().is_some() {
                                    this.home = None;
                                }
                                this.settle_key(window, cx);
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
        if let Draft::Review(review) = &draft
            && drafts::is_empty_review(review)
            && !self.saved.iter().any(|saved| saved.key() == draft.key())
        {
            self.save_status = "No local draft yet".into();
            return None;
        }
        let form = match draft {
            Draft::Pull(form) => form,
            other => return Some(other),
        };
        // The form is saved under the key its text belongs to and under no other, whatever
        // its fields name meanwhile. Text with no key takes the one they name where no other
        // draft holds it, and is otherwise not saved, which the status says.
        let key = FormKey::of(&form);
        if self.home.is_none() {
            if self
                .stored(&key)
                .is_some_and(|saved| !same_text(saved, &form))
            {
                self.save_status = DRAFT_HELD.into();
                return None;
            }
            self.home = Some(key);
        }
        self.home.as_ref().map(|home| Draft::Pull(home.keep(&form)))
    }
    /// The creation draft the panel knows to be saved under `key`.
    fn stored(&self, key: &FormKey) -> Option<&NewPull> {
        self.saved.iter().find_map(|saved| match saved {
            Draft::Pull(saved) if key.names(saved) => Some(saved),
            _ => None,
        })
    }
    /// The draft saved under the key the form's text belongs to.
    fn stored_at_home(&self) -> Option<&NewPull> {
        self.home.as_ref().and_then(|home| self.stored(home))
    }
    /// Settles the key the New pull request form's fields name, by the one rule for whose
    /// text a key holds (decided 2026-11-16; it loses nothing in any order of edits).
    ///
    /// 1. A draft belongs to the key it was typed or restored under: its destination, head
    ///    and base ([`FormKey`]). Nothing is taken and nothing is saved for a key the three
    ///    fields only pass through. A key counts once its edit has settled, which is when
    ///    the edited field loses focus ([`Self::settle_key`]), and at each transition that
    ///    saves the form at once ([`Self::save_draft`]: Review submission…, Pull requests,
    ///    Refresh PRs, a pull request's opening, the panel's close); as the form opens
    ///    (`opening`); and as the saved drafts are read. Focus leaving the field settles an
    ///    edit, not the quiet period a save waits out: a pause in the middle of a name would
    ///    count the prefix it stopped at, `fix` on the way to `fix-2`, and the form's text
    ///    waits on no timer to be safe, by the next point.
    /// 2. Before the fields name another key, the form's text is saved under the key it
    ///    belongs to, where it differs from what is stored there ([`Self::keep_home`]): at
    ///    the first keystroke of the edit ([`Self::key_edited`]) and again here. The edit
    ///    saves nothing under the key it names.
    /// 3. At the settled key, a draft saved there is taken whole, its title, its description
    ///    and Draft or Ready, in place of what the form showed, which is safe under its own
    ///    key and in Recover drafts. With none saved there the form keeps its text, which
    ///    belongs to the new key from now on and is saved there at the next save. A form is
    ///    never filled from two drafts.
    /// 4. The form's status says a draft was restored, until the next edit
    ///    ([`DRAFT_RESTORED`]).
    /// 5. Text with no key, typed while the destination named no repository or before the
    ///    saved drafts were read, is saved nowhere: it takes no saved draft's place in the
    ///    store, and no saved draft takes its place in the form. The form keeps it, the saved
    ///    draft stays, and the status says the text is not saved and why ([`DRAFT_HELD`]),
    ///    until that draft is discarded, the fields name a free key or the form is emptied.
    ///    The window is all that holds it meanwhile, as the status says, so the panel keeps
    ///    it through whatever takes the form or its fields away ([`Self::leave_form`]).
    ///
    /// A form that opens with neither a title nor a description takes the draft saved under
    /// its key though the key is its own: an emptied form is never saved, so the draft it
    /// had is still there. While the form stays open, what the person empties stays empty.
    ///
    /// The form once took a saved draft at every keystroke of the three fields and saved
    /// itself under each key it named: typed towards `fix-2`, an empty form took `fix`'s
    /// draft on the way and saved it over `fix-2`'s; a form that opened on one head's draft,
    /// edited onto another head's, saved the first over the second; and a form holding only
    /// a title took the other draft's description under it and saved the mix there.
    fn settle_form(&mut self, opening: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.create {
            return;
        }
        self.naming = false;
        // A destination that names no repository names no key: the text keeps the one it has.
        let Ok(Draft::Pull(form)) = self.current_draft(cx) else {
            return;
        };
        let key = FormKey::of(&form);
        let empty = form.title.is_empty() && form.body.is_empty();
        let moved = self.home.as_ref() != Some(&key);
        if !moved && !(opening && empty) {
            return;
        }
        if moved {
            self.keep_home(window, cx);
        }
        let Some(saved) = self
            .stored(&key)
            .filter(|saved| !same_text(saved, &form))
            .cloned()
        else {
            // Nothing is saved there, or the very text the form holds.
            self.home = Some(key);
            return;
        };
        if self.home.is_none() && !empty {
            self.save_status = DRAFT_HELD.into();
            return;
        }
        self.home = Some(key);
        self.fill_form(&saved, window, cx);
        // What was reviewed is no longer what the form shows.
        self.confirm = None;
        self.restored = true;
        self.save_status = DRAFT_RESTORED.into();
    }
    /// Puts `saved`'s title, description and Draft or Ready in the form.
    fn fill_form(&mut self, saved: &NewPull, window: &mut Window, cx: &mut Context<Self>) {
        self.draft = saved.draft;
        self.title
            .update(cx, |input, cx| input.set_value(&saved.title, window, cx));
        self.body
            .update(cx, |input, cx| input.set_value(&saved.body, window, cx));
    }
    /// The New pull request form gives way to the list or to a pull request: the panel keeps
    /// what it held, whole, for the form to open on again ([`Self::create`]). Every route
    /// that stops showing the form goes through here, before anything else takes its fields:
    /// Pull requests, Refresh PRs, a pull request's opening and a creation's landing.
    ///
    /// The store is no place to take the text from again. Text may have no key, while the
    /// destination names no repository or while another draft holds the key the fields name
    /// ([`DRAFT_HELD`]); it may have changed since its last save, which a destination that
    /// names no repository holds back; its save may have failed; and its draft may have been
    /// discarded in Recover drafts since. Each of those statuses tells the person that
    /// keeping the window open keeps the text, and the description's field is the review
    /// body's too: with a pull request opened in between, the form once came back holding
    /// its title and no description, the text nowhere.
    ///
    /// What is kept stays in this panel, which is one captured repository's for as long as
    /// it lives, and keeping it saves nothing and takes nothing: the form's fields hold it
    /// again before the key they name is settled, so the one rule decides what it is saved
    /// under and whether a saved draft takes its place ([`Self::settle_form`]). It ends as
    /// the form's text does: edited away, saved under a key it gains, used up by its own
    /// creation ([`Self::use_up_draft`], which empties the form before it is left), or gone
    /// with the window.
    fn leave_form(&mut self, cx: &App) {
        if std::mem::take(&mut self.create) {
            self.left = Some(self.form_text(cx));
        }
    }
    /// What the form's fields hold, whole. They are the form's while it is shown.
    fn form_text(&self, cx: &App) -> FormText {
        FormText {
            title: self.title.read(cx).value().to_string(),
            body: self.body.read(cx).value().to_string(),
            draft: self.draft,
        }
    }
    /// Whether the form, shown or left, holds text the panel does not know to be saved as
    /// it is under the key it belongs to: the window is then all that holds it, and the
    /// panel is kept while closed whatever the warm cache's bounds ([`WarmPanels::retain`]).
    fn holds_unsaved(&self, cx: &App) -> bool {
        let text = match &self.left {
            _ if self.create => self.form_text(cx),
            Some(left) => left.clone(),
            None => return false,
        };
        !(text.title.is_empty() && text.body.is_empty())
            && !self.stored_at_home().is_some_and(|saved| {
                saved.title == text.title && saved.body == text.body && saved.draft == text.draft
            })
    }
    /// Puts `left` back in the form, whole, writing only a field that holds something else:
    /// one that kept its text keeps its cursor and its scroll with it.
    fn hold_again(&mut self, left: &FormText, window: &mut Window, cx: &mut Context<Self>) {
        self.draft = left.draft;
        if self.title.read(cx).value() != left.title.as_str() {
            self.title
                .update(cx, |input, cx| input.set_value(&left.title, window, cx));
        }
        if self.body.read(cx).value() != left.body.as_str() {
            self.body
                .update(cx, |input, cx| input.set_value(&left.body, window, cx));
        }
    }
    /// Saves the form's text under the key it belongs to, at once, where it differs from what
    /// is stored there: before its fields name another key ([`Self::settle_form`], point 2).
    /// A snapshot waiting out the quiet period is of that text and goes. Nothing is saved of
    /// a form with neither a title nor a description, nor of text that has no key.
    fn keep_home(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.draft_timer = None;
        self.deferred_draft.clear();
        let Some(home) = self.home.clone() else {
            return;
        };
        let own = NewPull {
            repository: home.repository.clone(),
            title: self.title.read(cx).value().to_string(),
            body: self.body.read(cx).value().to_string(),
            head: home.head.clone(),
            base: home.base.clone(),
            draft: self.draft,
            expected_head: None,
            expected_base: None,
        };
        if own.title.is_empty() && own.body.is_empty() {
            return;
        }
        if self
            .stored(&home)
            .is_some_and(|saved| same_text(saved, &own))
        {
            return;
        }
        self.persist_draft(Draft::Pull(own), window, cx);
    }
    /// An edit of the destination, the head or the base: with the form open, its text is
    /// saved under the key it belongs to before the fields name another
    /// ([`Self::keep_home`]), and nothing is taken or saved for the key they name now, which
    /// counts once the field is left ([`Self::settle_key`]). With no form open the edit
    /// defers a save as any other does.
    fn key_edited(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.create {
            self.defer_draft_save(window, cx);
            return;
        }
        let restored = std::mem::take(&mut self.restored);
        self.naming = true;
        self.keep_home(window, cx);
        match self.current_draft(cx) {
            Err(issue) => {
                self.save_status = issue.save_message().into();
                if self.draft_issue.is_some() {
                    self.draft_issue = Some(issue);
                }
            }
            Ok(form) => {
                self.draft_issue = None;
                // A status that spoke of the key the fields named until now speaks of what
                // holds: the text is stored under its own key, or waits for the key it gets.
                let stale = restored
                    || [DRAFT_HELD, DraftIssue::Destination.save_message()]
                        .contains(&self.save_status.as_ref());
                if stale {
                    self.save_status = if empty_pull_draft(&form) {
                        ""
                    } else if self.home.is_some() {
                        "Draft saved locally"
                    } else {
                        "Saving local draft…"
                    }
                    .into();
                }
            }
        }
        cx.notify();
    }
    /// The field whose edit named another key lost focus, or the saved drafts were read: the
    /// key counts ([`Self::settle_form`]). Text that had no key until now is saved at once
    /// under the one it got, since it is saved nowhere else.
    fn settle_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.create {
            return;
        }
        let keyless = self.home.is_none();
        self.settle_form(false, window, cx);
        if keyless {
            self.save_form(window, cx);
        }
        cx.notify();
    }
    /// Saves the form or the review at once, where there is something to save: nothing of a
    /// form that shows a restored draft nothing has edited since, which holds what is stored
    /// and whose status stays.
    fn save_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(draft) = self.draft_to_save(cx)
            && !(self.restored && self.holds_stored(&draft))
        {
            self.persist_draft(draft, window, cx);
        }
    }
    fn defer_draft_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.restored = false;
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
        // The transition acts on the form as it stands, so the key its fields name counts
        // ([`Self::settle_form`]).
        self.settle_form(false, window, cx);
        self.save_form(window, cx);
        cx.notify();
    }
    /// Whether `draft` is a creation draft the panel knows to be saved as it is, under its
    /// key.
    fn holds_stored(&self, draft: &Draft) -> bool {
        matches!(draft, Draft::Pull(form) if self
            .stored(&FormKey::of(form))
            .is_some_and(|saved| same_text(saved, form)))
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
                    draft.into(),
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
        // Whatever is saved from here on is no draft the form has just taken.
        self.restored = false;
        let key = draft.key();
        if matches!(&draft, Draft::Reply(reply) if reply.body.is_empty())
            || matches!(&draft, Draft::Review(review) if drafts::is_empty_review(review))
        {
            // Empty replies and cleared reviews are serialized deletions, not
            // recoverable snapshots. Queue the deletion below while removing it
            // from the visible cache.
            self.saved.retain(|saved| saved.key() != key);
        } else if let Some(old) = self.saved.iter_mut().find(|d| d.key() == key) {
            *old = draft.clone();
        } else {
            self.saved.push(draft.clone());
        }
        let Ok(response) = self.owner.update(cx, |owner, _| {
            self.saver.queue_with(
                &owner.preferences_writer,
                key,
                draft.into(),
                drafts::save_batch,
            )
        }) else {
            self.save_status = "Draft save failed — keep this window open or copy the text".into();
            cx.notify();
            return;
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
                    // Nor does the save of what a form held before it took a saved draft
                    // replace what its status says of that draft, nor any save claim that
                    // text with no key was saved; a failed save is said either way.
                    let kept = this.create
                        && succeeded
                        && (this.restored
                            || (this.home.is_none()
                                && matches!(&current, Ok(form) if !empty_pull_draft(form))));
                    if !kept {
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
                    }
                    cx.notify();
                });
            })
            .detach();
        }
        cx.notify();
    }
    /// A creation GitHub confirmed has used up its draft: exactly the draft `sent` was written
    /// in leaves the draft store, as a confirmed reply's does, through the same saver as
    /// typing, so the removal replaces a snapshot of the sent text still queued under its key
    /// and lands after one being written ([`drafts::Change::Created`]). Recover drafts stops
    /// counting it at once, and the form gives up the title and the description where it
    /// still holds what was sent, so New pull request opens empty for that head and base.
    /// Left there, the form reopened on the text just sent, and sending it again could only
    /// be refused. Draft or Ready stays as the person chose it: a choice, not text that was
    /// sent. Nothing else goes: another head's, base's or repository's draft, and text under
    /// the same key that was not sent, are the person's ([`Draft::sent_as`]). A removal the
    /// store refuses keeps the draft on disk, which loses nothing.
    fn use_up_draft(&mut self, sent: &NewPull, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .deferred_draft
            .draft
            .as_ref()
            .is_some_and(|draft| draft.sent_as(sent))
        {
            self.draft_timer = None;
            self.deferred_draft.clear();
        }
        if self.current_draft(cx).is_ok_and(|form| form.sent_as(sent)) {
            self.title
                .update(cx, |input, cx| input.set_value("", window, cx));
            self.body
                .update(cx, |input, cx| input.set_value("", window, cx));
            self.save_status = SharedString::default();
            self.restored = false;
        }
        self.saved.retain(|saved| !saved.sent_as(sent));
        let key = Draft::Pull(sent.clone()).key();
        let used = drafts::Change::Created(Box::new(sent.clone()));
        if let Ok(response) = self.owner.update(cx, |owner, _| {
            self.saver
                .queue_with(&owner.preferences_writer, key, used, drafts::save_batch)
        }) {
            // A window closed next waits for the removal as it does for a save.
            let _ = track_save_completion(&mut self.save_completion, response);
        }
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
                // A Send refused at the foot of what it sends answers on the top line, out of
                // view from there: the line is shown and takes focus, as a failed send's is
                // ([`Self::focus_failure`]). A reply's Send is answered in its composer.
                if kind == RequestKind::Send && !self.answers_in_reply() {
                    self.focus_failure(window, cx);
                }
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
        self.existing = None;
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
                // A refusal that answered Escape while this request ran said to wait for its
                // result ([`Self::close`]), so its line goes before the result is received:
                // left there, it stood under "Pull request #N created." and was said again
                // as this request's answer, as if the send had not worked.
                let refused = this.refused_during.take() == Some(generation);
                let cleared = refused && this.error.as_deref() == Some(SEND_IN_FLIGHT);
                if cleared {
                    this.error = None;
                }
                // Whether that line, a stop, held focus as it went.
                let on_line = cleared && this.review.failure_focus.is_focused(window);
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
                // Focus the refusal's line held goes where the result put it, the new row or
                // the failure's own line; where the result moved it nowhere and left no line
                // for it to rest on, it goes to the content the panel shows now, before the
                // frame that draws the result, with the keys then down held off it
                // ([`Self::hand_focus_to_content`]). Left to the guard for focus on nothing
                // drawn, it moved a frame later, unheld.
                if on_line && this.error.is_none() && this.review.failure_focus.is_focused(window) {
                    this.hand_focus_to_content(window, cx);
                }
                // An Escape this request refused that is still down closes nothing at its
                // next repeat, which finds the request ended ([`HeldKeys::latch_escape`]).
                if refused {
                    this.held.borrow_mut().latch_escape();
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
    /// How the request being started opens its transport ([`Opener`]).
    fn opener(&self) -> Opener {
        Opener {
            fixture: self.review.fixture,
            moved: self.review.fixture_moved,
            #[cfg(test)]
            scripted: self.scripted.clone(),
        }
    }
    fn connect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.review.fixture {
            self.notice=Some("Offline fixture account: offline-reviewer. No credential or network access is available.".into());
            cx.notify();
            return;
        }
        let parent = account::capture_parent_window(cx);
        self.run(
            RequestKind::Connect,
            move |control| {
                not_cancelled(&control)?;
                account::connect(&control, parent)
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
        let parent = account::capture_parent_window(cx);
        self.run(
            RequestKind::Disconnect,
            move |control| {
                not_cancelled(&control)?;
                account::remove(&control, parent)
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
        let opener = self.opener();
        let query = self.pr_query.clone();
        self.run(
            RequestKind::Load,
            move |control| {
                let transport = opener.open()?;
                let login = transport.login().to_owned();
                let page = Client(transport).list_with_query(&repository, page, &query, &control)?;
                Ok((page, login, repository))
            },
            |result, this, window, cx| match result {
                Ok((page, login, repository)) => {
                    let held = window.focused(cx).is_some_and(|focus| {
                        (this.confirm.is_some()
                            && (focus == this.conversations.confirm_focus
                                || ["github-confirm-send", "github-edit-submission"]
                                    .into_iter()
                                    .any(|id| {
                                        this.review.buttons.kept(id).as_ref() == Some(&focus)
                                    })))
                            || (this.create
                                && (focus == this.title.read(cx).focus_handle(cx)
                                    || focus == this.head.read(cx).focus_handle(cx)
                                    || focus == this.base.read(cx).focus_handle(cx)
                                    || focus == this.body.read(cx).focus_handle(cx)
                                    || [
                                        "github-create-back",
                                        "Draft",
                                        "Ready",
                                        "github-load-templates",
                                        "github-review-submit",
                                    ]
                                    .into_iter()
                                    .any(|id| {
                                        this.review.buttons.kept(id).as_ref() == Some(&focus)
                                    })
                                    || (0..this.templates.len()).any(|index| {
                                        this.review
                                            .buttons
                                            .kept(("github-template", index))
                                            .as_ref()
                                            == Some(&focus)
                                    })))
                    });
                    this.leave_form(cx);
                    this.review.show_list = true;
                    this.confirm = None;
                    this.rows = page.items;
                    this.listed = Some(repository);
                    this.created_only = false;
                    this.page = page.page;
                    this.has_next = page.has_next;
                    this.account = Some(login);
                    this.notice = Some(
                        "PR list refreshed. Selecting a PR explicitly loads its current review and check status."
                            .into(),
                    );
                    if held {
                        this.hand_focus_to_content(window, cx);
                    }
                }
                Err(error) => this.error = Some(format!("{error:#}")),
            },
            window,
            cx,
        );
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
        let opener = self.opener();
        let mermaid_theme = if gpui_kit::component::Theme::global(cx).mode.is_dark() {
            gitturtle_preview::mermaid::MermaidTheme::Dark
        } else {
            gitturtle_preview::mermaid::MermaidTheme::Light
        };
        self.run(RequestKind::Load,move|control|{
            let mut client=Client(opener.open()?);
            let pull=client.pull(&repository,pull.number,&control)?;
            let captured=pull.capture(repository);
            let status=client.status(&captured,&control);
            let files=client.files(&captured,1,&control);
            let comments=client.review_threads(&captured,None,&control);
            // Prepare rendered Markdown & Mermaid diagrams off the UI thread.
            let rendered_body = pull.body.as_deref().and_then(|body| {
                crate::markdown_view::prepare_inline(body, mermaid_theme, || {
                    anyhow::ensure!(!control.is_cancelled(), "Pull request load cancelled");
                    Ok(())
                })
            });
            Ok((pull,captured,status,files,comments,rendered_body))
        },|result,this,window,cx|match result {
            Ok((pull,captured,status,files,comments,rendered_body))=>{
                this.leave_form(cx);this.confirm=None;this.review.reset_content();this.conversations.reset_content();this.review.show_list=false;this.review.section=Section::Overview;
                this.description=pull.body.as_ref().map(|body|text::wrapped_editor(body,"markdown",window,cx));
                this.description_view = rendered_body.map(|comparison| {
                    cx.new(|cx| crate::markdown_view::View::new(comparison, true, cx))
                });
                this.description_source = false;
                this.checks_expanded = false;
                let mut errors=vec![];
                match status {Ok(status)=>this.status=Some(status),Err(error)=>{this.status=None;errors.push(format!("Review/check status: {error:#}"));}}
                match files {Ok(page)=>{this.review.files=page.items;this.review.file_page=page.page;this.review.files_next=page.has_next;},Err(error)=>errors.push(format!("Changed files: {error:#}"))}
                match comments {Ok(page)=>{this.receive_threads(page,None);},Err(error)=>errors.push(format!("Discussions: {error:#}"))}
                this.pull=Some(pull);
                let draft=this.saved.iter().find_map(|draft|match draft {Draft::Review(review) if review.pull==captured=>Some(review.clone()),_=>None});
                this.body.update(cx,|input,cx|input.set_value(draft.as_ref().map(|d|d.body.clone()).unwrap_or_default(),window,cx));
                this.reviewed=true;
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
        // The description's field is the review body's too: what a review left in it goes,
        // and what the form itself left there stays. Cleared each time the form opened, and
        // the title then saved alone over the draft just saved, a description typed before
        // Pull requests was gone from the form and from the store as New pull request came
        // back to it, and one the store could not take was gone with the field.
        let reviewed = std::mem::take(&mut self.reviewed) || self.pull.is_some();
        self.create = true;
        self.conversations.reset_content();
        self.review.reset_content();
        self.review.comments.clear();
        self.review.composing = None;
        self.pull = None;
        self.status = None;
        self.description = None;
        self.description_view = None;
        self.description_source = false;
        self.checks_expanded = false;
        self.confirm = None;
        match self.left.take() {
            // A review took the description's place, no edit of the person's: the form
            // holds again what it was left with, whole, whether or not the store holds it
            // ([`Self::leave_form`]).
            Some(left) => self.hold_again(&left, window, cx),
            None if reviewed => self
                .body
                .update(cx, |input, cx| input.set_value("", window, cx)),
            None => {}
        }
        // The key the fields name counts as the form opens ([`Self::settle_form`]).
        self.restored = false;
        self.settle_form(true, window, cx);
        self.notice=Some("Use a branch already published to GitHub. Creating a PR never pushes a branch. Choose Draft or Ready, then review the destination.".into());
        self.save_draft(window, cx);
        cx.notify();
    }

    fn captured_pull(&self, cx: &App) -> Result<github::CapturedPull, String> {
        let pull = self
            .pull
            .as_ref()
            .ok_or_else(|| "Open a pull request first.".to_owned())?;
        let repository = self.destination(cx).map_err(|err| err.to_string())?;
        Ok(pull.capture(repository))
    }

    fn queue_pr_action(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        let Some(account) = self.account.clone() else {
            self.error =
                Some("Connect a GitHub account before performing outbound PR actions.".into());
            cx.notify();
            return;
        };
        self.confirm = Some(Confirmation { action, account });
        self.error = None;
        self.conversations.confirm_focus.focus(window, cx);
        cx.notify();
    }

    fn open_pr_in_browser(&mut self, cx: &mut Context<Self>) {
        let Some(pull) = self.pull.as_ref() else {
            return;
        };
        let Ok(repo) = self.destination(cx) else {
            return;
        };
        let url = pull.web_url(&repo);
        if url.starts_with("https://github.com/") {
            cx.open_url(&url);
        }
    }

    fn checkout_pr_branch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.review.fixture {
            return;
        }
        let captured = match self.captured_pull(cx) {
            Ok(captured) => captured,
            Err(err) => {
                self.error = Some(err);
                cx.notify();
                return;
            }
        };
        let number = captured.number;
        let expected_head_sha = captured.head.sha.clone();
        let expected_github_repo = Some(format!(
            "{}/{}",
            captured.repository.owner, captured.repository.name
        ));
        let worktree_path = self.repo.path().to_path_buf();
        let default_local = format!("pr-{number}");
        let owner = self.owner.clone();
        let default_remote = owner
            .upgrade()
            .map(|app| app.read(cx).remote_name.read(cx).value().trim().to_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "origin".into());
        let remote_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("origin")
                .default_value(default_remote)
        });
        let branch_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("pr-123")
                .default_value(default_local)
        });
        let error_slot: std::rc::Rc<std::cell::RefCell<Option<String>>> =
            std::rc::Rc::new(std::cell::RefCell::new(None));
        branch_input.focus_handle(cx).focus(window, cx);
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            let p = palette(cx);
            let labels = crate::appearance::labels(cx);
            let remote_input = remote_input.clone();
            let branch_input = branch_input.clone();
            let submit_remote = remote_input.clone();
            let submit_branch = branch_input.clone();
            let error_slot = std::rc::Rc::clone(&error_slot);
            let submit_error = std::rc::Rc::clone(&error_slot);
            let current_error = error_slot.borrow().clone();
            let owner = owner.clone();
            let worktree_path = worktree_path.clone();
            let expected_head_sha = expected_head_sha.clone();
            let expected_github_repo = expected_github_repo.clone();
            dialog
                .title(format!("Check out PR #{number} locally"))
                .width(appearance::dialog_width(px(480.), window))
                .max_h(appearance::dialog_max_height(window))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(
                            div()
                                .text_size(appearance::ui_text(12.))
                                .text_color(rgb(p.muted))
                                .child(format!(
                                    "Explicitly fetch refs/pull/{number}/head from the remote, verify it matches captured commit {}, and create a new local branch there.",
                                    short_oid(&expected_head_sha)
                                )),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child("Remote")
                                .child(Input::new(&remote_input).aria_label("Git remote name")),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child("New local branch name")
                                .child(Input::new(&branch_input).aria_label("New local branch name")),
                        )
                        .when_some(current_error, |col, err| {
                            col.child(
                                div()
                                    .id("checkout-pr-error")
                                    .alert(err.clone())
                                    .text_size(appearance::ui_text(11.))
                                    .text_color(rgb(labels.warning))
                                    .child(err),
                            )
                        }),
                )
                .footer(crate::tags::dialog_footer("Fetch & switch", Some("Cancel")))
                .on_ok(move |_, window, cx| {
                    let remote = submit_remote.read(cx).value().trim().to_owned();
                    let local_branch = submit_branch.read(cx).value().trim().to_owned();
                    if remote.is_empty() || local_branch.is_empty() {
                        *submit_error.borrow_mut() =
                            Some("Enter both a remote name and a new local branch name.".into());
                        window.refresh();
                        return false;
                    }
                    let _ = owner.update(cx, |this, cx| {
                        if this.path.as_ref() != Some(&worktree_path) {
                            this.operation_error = Some(
                                "Switched repository tab before checkout started; reopen Pull requests in the target repository."
                                    .into(),
                            );
                            cx.notify();
                            return;
                        }
                        this.write(
                            gitturtle_core::WriteCommand::CheckoutPullRequest {
                                remote,
                                number,
                                local_branch,
                                expected_head_sha: expected_head_sha.clone(),
                                expected_github_repo: expected_github_repo.clone(),
                            },
                            "Fetching PR branch…",
                            window,
                            cx,
                        );
                    });
                    true
                })
        });
    }

    fn open_merge_pr_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let captured = match self.captured_pull(cx) {
            Ok(captured) => captured,
            Err(err) => {
                self.error = Some(err);
                cx.notify();
                return;
            }
        };
        let default_title = self
            .pull
            .as_ref()
            .map(|p| format!("{} (#{})", p.title, p.number))
            .unwrap_or_default();
        let method = std::rc::Rc::new(std::cell::Cell::new(github::MergeMethod::Squash));
        let title_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Optional merge commit title")
                .default_value(default_title)
        });
        let panel = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            let p = palette(cx);
            let method = std::rc::Rc::clone(&method);
            let submit_method = std::rc::Rc::clone(&method);
            let title_input = title_input.clone();
            let submit_title = title_input.clone();
            let panel = panel.clone();
            let captured = captured.clone();
            let selected = method.get();
            dialog
                .title(format!("Merge PR #{}", captured.number))
                .width(appearance::dialog_width(px(480.), window))
                .max_h(appearance::dialog_max_height(window))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(
                            div()
                                .text_size(appearance::ui_text(12.))
                                .text_color(rgb(p.muted))
                                .child(format!(
                                    "Pinned to captured head {} so a remote push cannot merge unreviewed commits.",
                                    short_oid(&captured.head.sha)
                                )),
                        )
                        .child(
                            div().flex().flex_col().gap_1().children(
                                github::MergeMethod::ALL.into_iter().map(|choice| {
                                    let active = selected == choice;
                                    let method = std::rc::Rc::clone(&method);
                                    button(
                                        ("github-merge-method", choice as usize),
                                        choice.label(),
                                        if active { "check" } else { "" },
                                        active,
                                    )
                                    .toggled(active)
                                    .accessibility_label(format!("Merge method: {}", choice.label()))
                                    .w_full()
                                    .on_click(move |_, window, _| {
                                        method.set(choice);
                                        window.refresh();
                                    })
                                }),
                            ),
                        )
                        .when(selected != github::MergeMethod::Rebase, |col| {
                            col.child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child("Commit title (optional)")
                                    .child(Input::new(&title_input).aria_label("Merge commit title")),
                            )
                        }),
                )
                .footer(crate::tags::dialog_footer("Review merge…", Some("Cancel")))
                .on_ok(move |_, window, cx| {
                    let chosen = submit_method.get();
                    let raw_title = submit_title.read(cx).value().trim().to_owned();
                    let commit_title = (!raw_title.is_empty()
                        && chosen != github::MergeMethod::Rebase)
                        .then_some(raw_title);
                    let action = Action::MergePull {
                        pull: captured.clone(),
                        method: chosen,
                        commit_title,
                        commit_message: None,
                    };
                    let _ = panel.update(cx, |this, cx| {
                        this.queue_pr_action(action, window, cx);
                    });
                    true
                })
        });
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "PR CSV editor dialog takes title, prompt, placeholder, initial value, empty-input policy, builder, window, and context"
    )]
    fn open_pr_csv_dialog(
        &mut self,
        title: &'static str,
        prompt: &'static str,
        placeholder: &'static str,
        initial: String,
        allow_empty: bool,
        build_action: impl Fn(github::CapturedPull, Vec<String>) -> Action + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let captured = match self.captured_pull(cx) {
            Ok(captured) => captured,
            Err(err) => {
                self.error = Some(err);
                cx.notify();
                return;
            }
        };
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder)
                .default_value(initial)
        });
        input.focus_handle(cx).focus(window, cx);
        let error_slot: std::rc::Rc<std::cell::RefCell<Option<String>>> =
            std::rc::Rc::new(std::cell::RefCell::new(None));
        let panel = cx.entity().downgrade();
        let build_action = std::rc::Rc::new(build_action);
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            let p = palette(cx);
            let labels = crate::appearance::labels(cx);
            let input = input.clone();
            let submit_input = input.clone();
            let error_slot = std::rc::Rc::clone(&error_slot);
            let submit_error = std::rc::Rc::clone(&error_slot);
            let current_error = error_slot.borrow().clone();
            let panel = panel.clone();
            let captured = captured.clone();
            let build_action = std::rc::Rc::clone(&build_action);
            dialog
                .title(title)
                .width(appearance::dialog_width(px(480.), window))
                .max_h(appearance::dialog_max_height(window))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .text_size(appearance::ui_text(12.))
                                .text_color(rgb(p.muted))
                                .child(prompt),
                        )
                        .child(Input::new(&input).aria_label(title))
                        .when_some(current_error, |col, err| {
                            col.child(
                                div()
                                    .id("pr-csv-error")
                                    .alert(err.clone())
                                    .text_size(appearance::ui_text(11.))
                                    .text_color(rgb(labels.warning))
                                    .child(err),
                            )
                        }),
                )
                .footer(crate::tags::dialog_footer("Review change…", Some("Cancel")))
                .on_ok(move |_, window, cx| {
                    let values: Vec<String> = submit_input
                        .read(cx)
                        .value()
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(ToOwned::to_owned)
                        .collect();
                    if !allow_empty && values.is_empty() {
                        *submit_error.borrow_mut() =
                            Some("Enter at least one GitHub username.".into());
                        window.refresh();
                        return false;
                    }
                    let action = build_action(captured.clone(), values);
                    let _ = panel.update(cx, |this, cx| {
                        this.queue_pr_action(action, window, cx);
                    });
                    true
                })
        });
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
        let opener = self.opener();
        if let Draft::Pull(pull) = draft {
            self.run(
                RequestKind::Load,
                move |control| {
                    let transport = opener.open()?;
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
        let opener = self.opener();
        let sent_action = confirmation.action.clone();
        // A reply's Send answers in the reply's composer, where the reply was written; every
        // other action's, a conversation's Resolve or Reopen included, on the panel's top line.
        let origin = match &confirmation.action {
            Action::Reply { target, .. } => Origin::Reply(Box::new(target.clone())),
            _ => Origin::Panel,
        };
        let sent = self.run_from(origin, RequestKind::Send, move |control| {
            let transport = opener.open()?;
            anyhow::ensure!(transport.login() == confirmation.account, "The connected GitHub account changed. Review the destination again before submitting.");
            let destination = format!("{} · account {}", confirmation.action.destination(), confirmation.account);
            drafts::record_attempt(destination.clone(), "Submission started; if no final outcome appears, inspect GitHub before sending again.".into())?;
            let completed_reply = match &confirmation.action {
                Action::Reply { target, body } => Some(drafts::ReplyDraft { target: target.clone(), body: body.clone() }),
                _ => None,
            };
            let result = Client(transport).execute(confirmation.action, &control);
            let outcome = match &result { Ok(sent) => sent.notice.clone(), Err(error) => format!("{error:#}") };
            if let Err(error) = drafts::record_reply_outcome(destination, outcome.chars().take(4000).collect(), completed_reply.as_ref().filter(|_|result.is_ok())) {
                return Err(anyhow::anyhow!("The GitHub action finished but saving its outcome failed: {error}. Check GitHub before resubmitting."));
            }
            result
        }, move |result, this, window, cx| match result {
            Ok(sent) => {
                if matches!(sent_action, Action::Reply {..} | Action::SetThreadResolved {..}) {
                    this.finish_conversation_action(&sent_action, window, cx);
                } else if let (Action::Create(new), Some(created)) = (&sent_action, sent.created) {
                    this.show_created(new, created, window, cx);
                } else {
                    this.notice = Some(format!("{}. The local draft remains available; submitting it again creates another outbound action.", sent.notice));
                }
            },
            Err(error) => {
                let exists = matches!(sent_action, Action::Create(_)) && error.is::<github::PullExists>();
                this.error = Some(match &sent_action {
                    // The one refusal whose outcome is certain: the panel says what it found,
                    // from the head it captured, in place of the sentence every other one gets.
                    Action::Create(new) if exists => this.refused_for_existing(new),
                    _ => format!("{error:#}"),
                });
                // One rule for where focus rests once a send has failed: on the remedy the
                // failure's line offers, and otherwise on that line itself.
                if exists { this.focus_refusal(window, cx); }
                else if matches!(sent_action, Action::Reply { .. }) {
                    // A reply's failure is drawn in its composer, where focus goes back to
                    // the draft; one whose composer has gone answers on the top line.
                    this.show_line.set(true);
                    if this.conversations.active.is_some() { this.focus_visible_section(window,cx); }
                }
                else { this.focus_failure(window, cx); }
            },
        }, window, cx);
        if sent {
            // The confirmation goes with its controls, so focus one of them holds, as Send
            // does once pressed, goes to the content the panel shows, as it does when Keep
            // editing takes the confirmation away. Left on Send, which no frame then drew,
            // nothing showed focus and the window's keys reached nothing of the panel: the
            // Return's own key-up never reached the toolkit's record of the press, and the
            // next Return in the panel, taken for its repeat, did nothing.
            let held = window.focused(cx).is_some_and(|focus| {
                focus == self.conversations.confirm_focus
                    || ["github-confirm-send", "github-edit-submission"]
                        .into_iter()
                        .any(|id| self.review.buttons.kept(id).as_ref() == Some(&focus))
            });
            self.confirm = None;
            if held {
                self.hand_focus_to_content(window, cx);
            }
        }
    }
    /// The panel's one hand-off of focus from a control that goes to the content it shows
    /// ([`Self::focus_visible_section`]): from Send to GitHub or Keep editing as the
    /// confirmation goes with its action sent ([`Self::submit`]), and from an Escape
    /// refusal's line as it goes with the send it refused during ([`Self::run_from`]).
    fn hand_focus_to_content(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The hand-off shows nothing new, so the body shows nothing anew: the focus moves,
        // and the body's record of the focus it has shown moves with it. Revealed, the
        // reply's draft scrolled back into view as its Send started.
        let shown = std::mem::take(&mut *self.review.revealed_focus.borrow_mut());
        self.focus_visible_section(window, cx);
        *self.review.revealed_focus.borrow_mut() = review::FocusObservation {
            focus: window.focused(cx),
            ..shown
        };
        // A key still down as focus arrives, a Return that sent going down, under X11 a
        // Space whose repeat's release sent mid-hold, or one pressed on the refusal's line:
        // its press is latched, so it types nothing into the field focus went to
        // ([`HeldKeys`]).
        if let Some(focus) = window.focused(cx) {
            self.land_focus(&focus, window, cx);
        }
    }
    /// Hears Return and Space go down in `window`, the panel's, while the panel is open
    /// ([`HeldKeys`]): with focus where the panel hears them come up, the key's press begins
    /// or repeats; anywhere else, as in a dialog over the panel, a key of the panel still down
    /// may come up unheard ([`HeldKeys::leave`]). The panel's close drops it,
    /// so a closed panel, kept warm for its repository, takes no part in the window's keys.
    ///
    /// It stops one key-down and no other: Return's or Space's, heard in the panel, while that
    /// key's press is latched and a text field of the panel holds focus ([`FIELD`]). A latched
    /// press was down as a landing or a send's hand-off moved focus and has not come up since
    /// ([`Panel::land_focus`]), so the key-down is a repeat of the press that pressed Send, or
    /// of a key pressed while that one was down, and never a press the person aimed at the
    /// field: a key they press in a field they are typing in begins its own press, unlatched,
    /// and repeats there. Stopped, the key-down reaches no binding and types no character:
    /// Space held on Send to GitHub, which under X11 sends at a repeat's release mid-hold,
    /// typed a space into the title, the review body or the reply at each later repeat once
    /// the send had ended, and Return a line where a repeat's release and press came in two
    /// batches. On a control that is no text field nothing is stopped: a latched press's
    /// click there is refused where it would act ([`HeldKeys::refuses`]).
    ///
    /// It hears Escape too, and stops its key-down wherever in the panel it is heard while
    /// its press is latched ([`HeldKeys::latch_escape`]): the dialog's Cancel, which closes
    /// the panel, and a field's own Escape take none of it.
    fn hear_keys(&mut self, window: crate::gpui::WindowId, cx: &mut Context<Self>) {
        let held = self.held.clone();
        self.hears_keys = Some(cx.intercept_keystrokes(move |event, pressed_in, cx| {
            let Some(own) = heard_key(&event.keystroke) else {
                return;
            };
            if pressed_in.window_handle().window_id() != window {
                return;
            }
            let heard = event
                .context_stack
                .iter()
                .any(|context| context.contains(review::BODY) || context.contains(HELD));
            let mut held = held.borrow_mut();
            if own == ESCAPE {
                if heard && held.escape_down() {
                    cx.stop_propagation();
                }
                return;
            }
            if heard {
                held.key_down(own, key_up_withheld(&event.keystroke));
                let in_field = event
                    .context_stack
                    .iter()
                    .any(|context| context.contains(FIELD));
                if in_field && held.keys[own].is_some_and(|press| press.latched) {
                    cx.stop_propagation();
                }
            } else {
                held.leave();
            }
        }));
    }
    /// Notes the key-up of a Return, Space or Escape press under way ([`HeldKeys::key_down`],
    /// [`HeldKeys::escape_down`]), before
    /// the control it comes up on takes its click, which a latched press refuses
    /// ([`HeldKeys::refuses`]): the press
    /// ends at the next frame, unless its key goes down again first. GPUI's X11 client drops
    /// a repeat's release when its press comes in the same batch of events, but a pair split
    /// across two batches reaches the panel as a key-up, an idle pass and a key-down; the next
    /// batch comes within microseconds, a frame some milliseconds later, and a person's next
    /// press later still. A press is down until then, so a landing between the two halves of
    /// such a repeat finds the key down.
    fn note_key_up(
        &mut self,
        event: &gpui_kit::KeyUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(own) = heard_key(&event.keystroke) else {
            return;
        };
        let mut held = self.held.borrow_mut();
        let key_up = held.key_ups.wrapping_add(1);
        let Some(press) = held.press(own).as_mut() else {
            return;
        };
        press.released = Some(key_up);
        held.key_ups = key_up;
        let panel = cx.weak_entity();
        window.on_next_frame(move |_, cx| {
            let _ = panel.update(cx, |panel, _| panel.end_press(own, key_up));
        });
    }
    /// Ends the press of Return, Space or Escape that `key_up` released, unless its key went
    /// down again; the other keys' presses stay.
    fn end_press(&mut self, own: usize, key_up: u64) {
        let mut held = self.held.borrow_mut();
        let press = held.press(own);
        if press.is_some_and(|press| press.released == Some(key_up)) {
            *press = None;
        }
        if !held.latched() {
            held.watch = None;
        }
    }
    /// A landing gave `focus`, a control of the panel, focus the person did not move there:
    /// the presses of Return and Space then under way are latched, so no keyboard click acts
    /// on the controls a landing focuses until each has come up ([`HeldKeys`]). With none
    /// under way nothing is latched, and the person's next press acts.
    fn land_focus(&mut self, focus: &FocusHandle, window: &mut Window, cx: &mut Context<Self>) {
        let mut latched = false;
        for press in self.held.borrow_mut().keys.iter_mut().flatten() {
            press.latched = true;
            latched = true;
        }
        let watch = latched.then(|| {
            cx.on_focus_out(focus, window, |this, _, _, _| {
                this.held.borrow_mut().leave()
            })
        });
        self.held.borrow_mut().watch = watch;
    }
    /// Gives focus to the remedy a refused creation's line names ([`Self::refused_for_existing`]):
    /// its action, Open #N, where the line offers one, and otherwise Refresh PRs, drawn with
    /// its ring where the keyboard sent, so Tab goes on from there and Escape closes the
    /// panel; and holds the keys then down off it ([`Self::land_focus`]).
    fn focus_refusal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let remedy = if self.offered(cx).is_some() {
            "github-open-existing"
        } else {
            "github-refresh"
        };
        let focus = self.review.buttons.focus(remedy, cx);
        focus.focus(window, cx);
        // Shown whole with its ring whatever input sent the creation: the body's parts reveal
        // the control focus moves onto only after a key, and after a pointer's click on Send,
        // Refresh PRs, a part of the toolbar above the line, stood at the body's top edge with
        // the top of its ring cut.
        self.review.reopening.reveal();
        self.land_focus(&focus, window, cx);
    }
    /// Rests focus on the failure's line itself, on the panel's top line, as a send ends in a
    /// failure or a refusal whose line offers no remedy: a creation, a review, a comment and a
    /// conversation's Resolve or Reopen, at every size. One rule with [`Self::focus_refusal`]:
    /// focus goes where the answer is, on the remedy where the line names one and otherwise on
    /// the line, which is the next thing the person reads. The line is then a stop of its own
    /// ([`review::FailureStop`]): it draws its ring on its own edge, shown whatever input sent,
    /// since a pointer's click on Send leaves the window's ring unmarked and focus on a line
    /// that drew none was on nothing the person saw; Tab goes on to the control below it and
    /// Shift+Tab comes back; it takes no press, so Return and Space do nothing there and a key
    /// still held from Send presses nothing; and the body shows it whole wherever it had
    /// scrolled, in the frame that first draws it ([`Failed`]). A screen reader follows focus
    /// to the line and reads it there, once: the window's alert leaves that failure to it
    /// ([`Self::announce`]). The two refusals that answer a send there follow the same rule:
    /// Send to GitHub pressed while the window is busy ([`Self::run_from`]) and Escape pressed
    /// while the send is in flight ([`Self::close`]).
    ///
    /// Focus once stayed on the form's title field after a failed creation, or went to Pull
    /// requests where the view could not hold the line and the field, a ring that read
    /// "leave" where the person meant to fix and try again, and after a failed review,
    /// comment or Resolve stayed where sending left it, which the line's reveal could leave
    /// out of view.
    fn focus_failure(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_line.set(true);
        let focus = self.review.failure_focus.clone();
        self.review.failure_stop = review::FailureStop::Landed;
        // Focus that is on the line already moves nowhere, so nothing reads it: the alert does.
        self.review.failure_read = !focus.is_focused(window);
        focus.focus(window, cx);
        if let Some(owner) = self.owner.upgrade() {
            owner.read(cx).focus_ring.mark(window);
        }
    }
    /// Shows the pull request a creation of `new` made, from the creation's own answer and with
    /// no request after it. The notice names it, "Pull request #N created." or, for a draft,
    /// "Draft pull request #N created."; its row comes first in the list, as GitHub lists the
    /// newest first, in place of a row of the same number, drawn as any listed row is; and the
    /// row is the keyboard's place, shown whole in its list and in the body whatever input sent
    /// the creation. It is not opened: opening it reads from GitHub, which is the person's
    /// press. The notice once read "Pull request created. Refresh PRs to open it.", the list
    /// unchanged.
    ///
    /// The row joins the first page of `new`'s repository where that is what `rows` lists: a
    /// full page then gives up its last row, which GitHub now lists on the next page, so the
    /// next page read from there neither doubles nor skips a row. Where `rows` lists nothing,
    /// another repository or a later page, the new row stands alone on page 1 and the list
    /// says it shows only that ([`Panel::created_only`]), since a new row first on a later page
    /// would place the newest pull request there.
    ///
    /// An answer that held only the number names it and says Refresh PRs opens it, and one
    /// that could not be read says what it once said: the creation was accepted either way.
    ///
    /// An answer that names the pull request, whole or by its number, confirms the creation,
    /// and the creation's draft is used up ([`Self::use_up_draft`]). One that could not be
    /// read confirms nothing of what was made, so the draft stays whole, as it does through
    /// every failure and refusal: the text is the person's.
    fn show_created(
        &mut self,
        new: &NewPull,
        created: github::Created,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(created, github::Created::Unread) {
            self.use_up_draft(new, window, cx);
        }
        self.leave_form(cx);
        self.review.show_list = true;
        let pull = match created {
            github::Created::Pull(pull) => *pull,
            unlisted => {
                self.notice = Some(match unlisted {
                    github::Created::Number(number) => {
                        format!("Pull request #{number} created. Refresh PRs to open it.")
                    }
                    _ => "Pull request created. Refresh PRs to open it.".into(),
                });
                self.focus_visible_section(window, cx);
                return;
            }
        };
        let number = pull.number;
        let kind = if pull.draft {
            "Draft pull request"
        } else {
            "Pull request"
        };
        self.notice = Some(format!("{kind} #{number} created."));
        let listed = self.listed.as_ref() == Some(&new.repository) && self.page == 1;
        if listed {
            self.rows.retain(|row| row.number != number);
            self.rows.insert(0, pull);
            if self.rows.len() > github::PAGE_SIZE {
                self.rows.truncate(github::PAGE_SIZE);
                self.has_next = true;
            }
        } else {
            self.rows = vec![pull];
            self.page = 1;
            self.has_next = false;
            self.listed = Some(new.repository.clone());
        }
        self.created_only = !listed || self.created_only;
        // The row's title takes focus at the top of its list, whose first row it is, and the
        // frame that draws it shows the notice and the row together ([`Created`]).
        self.review.pull_scroll.set_offset(point(px(0.), px(0.)));
        self.show_row.set(true);
        let focus = self.review.buttons.focus(("github-pr", number), cx);
        focus.focus(window, cx);
        // A key still down from Send presses nothing on the row ([`Self::land_focus`]).
        self.land_focus(&focus, window, cx);
    }
    /// What the failure line says of a creation of `new` that GitHub refused because its head
    /// branch already has a pull request, in the app's own words and from the head the
    /// creation captured, none of GitHub's text: with the pull request's number, and Open #N as
    /// the line's one action, where the loaded list of that repository holds it
    /// ([`held_pull`]), and otherwise that Refresh PRs shows it. Nothing is looked up or
    /// refreshed for it: the next request is the person's press. The outcome is certain, so
    /// the line says "Nothing was created" where an uncertain write's says to check GitHub. The
    /// body shows the line wherever it has scrolled ([`Failed`]): Send stands at
    /// the form's foot, and the line once landed above the body's top, out of view, at 13 pt in
    /// a 461 × 490 window.
    fn refused_for_existing(&mut self, new: &NewPull) -> String {
        self.show_line.set(true);
        let held = self
            .listed
            .as_ref()
            .filter(|listed| **listed == new.repository)
            .and_then(|_| held_pull(&self.rows, new))
            .cloned();
        match held {
            Some(pull) => {
                let line = format!(
                    "{} already has pull request #{}. Nothing was created.",
                    new.head, pull.number
                );
                self.existing = Some(Existing {
                    line: line.clone(),
                    repository: new.repository.clone(),
                    pull,
                });
                line
            }
            None => format!(
                "{} already has a pull request. Nothing was created. Refresh PRs to see it.",
                new.head
            ),
        }
    }
    /// The pull request a refused creation's line offers to open ([`Existing`]): beside the
    /// line it was found for alone, and only while the destination field names the repository
    /// the creation was refused in. The field's edit takes it away, and this holds whatever
    /// reached the field without one.
    fn offered(&self, cx: &App) -> Option<&PullRequest> {
        self.existing
            .as_ref()
            .filter(|existing| self.error.as_ref() == Some(&existing.line))
            .filter(|existing| {
                self.destination(cx)
                    .is_ok_and(|destination| destination == existing.repository)
            })
            .map(|existing| &existing.pull)
    }
    /// Open #N's press: opens the pull request the line offers, as a press on its row does
    /// ([`Self::inspect`]), checked again as it is pressed, before any request, so it reads
    /// nothing once the destination names another repository.
    fn open_existing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pull) = self.offered(cx).cloned() else {
            cx.notify();
            return;
        };
        self.inspect(pull, window, cx);
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
        // A failure whose line took focus as it landed is read from that line, as a screen
        // reader follows focus there: said by the window's alert as well, it was read twice.
        let read = std::mem::take(&mut self.review.failure_read);
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
                !read && (shown.error != heard.error || shown.presses.error != heard.presses.error)
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
    /// whole height. The confirmation below it reads `flowing` too, for a change of the line's
    /// height ([`conversations::ConfirmFit::line_among`]).
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
            self.refuse(SEND_IN_FLIGHT);
            // The refusal says to wait for this send's result, and goes as it comes
            // ([`Self::run_from`]).
            self.refused_during = Some(self.request_generation);
            // Escape pressed from what is being sent answers on the top line, out of view
            // from there: the line is shown and takes focus, as a failed send's is.
            self.focus_failure(window, cx);
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
        // And a Return held off a review comment, whose key-up lands outside the panel.
        self.forget_return();
        // Closed, the panel takes no part in the window's keys ([`Self::hear_keys`]), and
        // keeps no press it heard go down: its key comes up outside the closed panel.
        self.hears_keys = None;
        *self.held.borrow_mut() = HeldKeys::default();
        self.hold_closing_focus(window, cx);
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
                unsaved: self.holds_unsaved(cx),
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
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(Ok(())) => {
                        this.saved.retain(|draft| draft.key() != key);
                        this.notice =
                            Some("The selected saved draft was discarded locally.".into());
                        // Text the discarded draft kept out of the store takes the key it
                        // gave up ([`Self::settle_form`], point 5).
                        if this.home.is_none() {
                            this.settle_key(window, cx);
                        }
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
impl Panel {
    /// The panel's one guard for focus a pane takes with it as it goes, run once a frame has
    /// drawn the window with focus on nothing it drew: where the Conversations pane, the
    /// connection pane or Overview's captured identities held that focus in the frame before, on the pane, as a click inside
    /// either leaves it, or on a control inside it, as Tab from there leaves it
    /// ([`review::ReviewState::pane_held`]), the control whose press took the pane away
    /// takes it ([`review::ReviewState::closer`]), as Account options
    /// does as it closes the connection pane, New pull request as it replaces the PR, a mode
    /// as it replaces Threads and a page of files as it loads, where the frame drew that
    /// control enabled; otherwise the content the panel shows takes it, as the file list does
    /// where the page loaded leaves the button that asked for it disabled
    /// ([`Panel::focus_visible_section`]). Left on a pane no frame drew, focus started the
    /// next Tab again from the window's first stop, and the panel's keys, Escape among them,
    /// went nowhere.
    fn hand_off_lost_pane_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let review = &mut self.review;
        // Focus lost anywhere else, or by a closed panel's panes, is not the panel's to move.
        let lost = window.focused(cx);
        if self.closed || lost.is_none() || review.pane_held.take() != lost {
            return;
        }
        let closer = review
            .closing
            .take()
            .filter(|closer| review.closers_drawn.get_mut().contains(closer));
        match closer {
            Some(closer) => closer.focus(window, cx),
            None => self.focus_visible_section(window, cx),
        }
        cx.notify();
    }
    /// Names the part of each PR row's title (`pr-{number}`) where the rows have changed since
    /// they were last named, as a new page of them does: a frame drawing the same rows
    /// allocates no names ([`Panel::row_parts`]).
    fn name_row_parts(&mut self) {
        let named = self.row_parts.len() == self.rows.len()
            && self
                .row_parts
                .iter()
                .zip(&self.rows)
                .all(|((number, _), pull)| *number == pull.number);
        if !named {
            self.row_parts = self
                .rows
                .iter()
                .map(|pull| (pull.number, format!("pr-{}", pull.number).into()))
                .collect();
        }
    }
}

impl Render for Panel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.announce(window, cx);
        self.name_row_parts();
        #[cfg(test)]
        review::PARTS_DRAWN.with(|parts| parts.borrow_mut().clear());
        // The controls this frame draws enabled that can take a pane away ([`Panel::closing`]),
        // and the one pressed since the last frame, which this frame alone may hand focus to.
        self.review.closers_drawn.get_mut().clear();
        self.review.closing = self.review.closer.take();
        // Where focus lies in the body, which this frame's parts record as they prepaint.
        self.review.reopening.begin_frame();
        // The body's Buttons' handles that the last frame drew ([`review::Buttons`]).
        self.review.buttons.begin_frame(window);
        // The focus a pane that can go with it holds in the frame the window last drew
        // ([`Panel::hand_off_lost_pane_focus`]).
        let review = &self.review;
        let held = [
            &review.section_focus,
            &review.connection_focus,
            &review.identities_focus,
            // The failure's line, a stop while it answers a send, goes as the line clears.
            &review.failure_focus,
        ]
        .into_iter()
        .any(|pane| pane.contains_focused(window, cx));
        self.review.pane_held = window.focused(cx).filter(|_| held);
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
            // The failure's line is a stop for the failure that put focus on it, until the
            // line answers another ([`review::FailureStop`]).
            if said.error != self.top_line_said.error
                || said.presses.error != self.top_line_said.presses.error
            {
                self.review.failure_stop = self.review.failure_stop.answered();
            }
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
        // The pull request a refused creation's line offers to open ([`Self::offered`]).
        let existing = self.offered(cx).map(|pull| pull.number);
        // A failure landed that the body must show ([`Self::show_line`]): this frame lays out
        // its line, and Refresh PRs where focus rests on it as the line's remedy, and the body
        // shows them before its content prepaints ([`Failed`]). Above the body the line is
        // always in view.
        let landed = (self.show_line.take() && !pinned).then(|| {
            let laid = || std::rc::Rc::new(std::cell::Cell::new(None));
            (laid(), laid())
        });
        let refresh_laid = landed.as_ref().map(|(_, remedy)| remedy).filter(|_| {
            self.review
                .buttons
                .kept("github-refresh")
                .is_some_and(|refresh| refresh.is_focused(window))
        });
        let line_laid = landed.as_ref().map(|(line, _)| line);
        // The failure's line as a stop of its own, for the failure that put focus on it and
        // for as long as it holds that focus ([`Self::focus_failure`]): the line tracks the
        // focus, so a screen reader that follows focus reads the line, and draws its ring on
        // its own edge, 1 px of accent laid over the line so nothing moves for it, as a pane
        // that is a stop does. Among the content it is a part of the body, which shows it
        // whole when the keyboard comes back to it; above the body it is in view already.
        let failure = self.review.failure_focus.clone();
        let stop = self.error.is_some()
            && (self.review.failure_stop != review::FailureStop::No || failure.is_focused(window));
        let ring = stop
            .then(|| self.owner.upgrade())
            .flatten()
            .and_then(|owner| owner.read(cx).focus_ring.edge(&failure, rgb(p.accent)));
        lines.extend(
            self.outcome("github-error", "github-cancelled", cx)
                .filter(|_| !self.answers_in_reply())
                .map(|line| {
                    let line = line.ml(text_inset());
                    let Some(number) = existing else {
                        if !stop {
                            return line.into_any_element();
                        }
                        return if pinned {
                            div().debug_selector(|| "github-part-failure".into())
                        } else {
                            self.ringed_part("failure", div(), &failure, cx)
                        }
                        .relative()
                        .child(line.track_focus(&failure))
                        .children(ring)
                        .into_any_element();
                    };
                    // The line's one action stands beside its words where the row holds both
                    // whole, and below them otherwise, at the panel's edge as its other Buttons
                    // are, the words wrapping in the row's width. Its press is the row's
                    // ([`Self::inspect`]): it sends the one read, which takes the line, and the
                    // action with it, away.
                    let open = format!("Open #{number}");
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_2()
                        .child(line.min_w_0())
                        .child(
                            self.part("open-existing", div(), cx).child(
                                // Its name says what #N is: read alone, as a screen reader
                                // lists a window's buttons, "Open #7" named nothing.
                                named_for_reading(
                                    self.button("github-open-existing", open, "", false, cx),
                                    "github-open-existing",
                                    format!("Open pull request #{number}"),
                                )
                                .debug_selector(|| "github-open-existing".into())
                                .disabled(pending)
                                .on_click(cx.listener(
                                    |this, event, window, cx| {
                                        if !this.held.borrow().refuses(event) {
                                            this.open_existing(window, cx)
                                        }
                                    },
                                )),
                            ),
                        )
                        .into_any_element()
                })
                .map(|row| laid_out(row, line_laid)),
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
            .debug_selector(|| "github-busy-row".into())
            .flex()
            .items_center()
            .gap_2()
            .pl(text_inset())
            .child(
                // Too long for the row, it wraps beside Cancel, which stays whole inside it.
                label("github-busy", self.request.label())
                    .min_w_0()
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
        // Overview's captured identities, which the last section tab owns, are drawn only on
        // a PR's Overview: elsewhere the tab owns nothing.
        if self.pull.is_none() || self.review.section != Section::Overview {
            self.review.identities.forget();
        }
        // A line the last frame stood above the body moves back among the content as the
        // conversation closes, and an element before the body moves the body's scroll by the
        // difference, read from where this frame lays the line out ([`Self::unpin_top_line`]).
        let stood = self.review.top_line_pinned.replace(pinned);
        let unpinned = stood && !pinned;
        // A creation landed: this frame lays out the notice's line and the list, which the
        // body shows together before its content prepaints ([`Created`]).
        let created = (self.show_row.take() && !self.create && !pinned && !unpinned).then(|| {
            let laid = || std::rc::Rc::new(std::cell::Cell::new(None));
            (laid(), laid())
        });
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
            let laid = created.as_ref().map(|(line, _)| line);
            (line.map(|line| laid_out(line, laid)), None)
        };
        // Each part of the content starts a block of the body ([`review::Blocks`]), and the
        // line among it records the room it takes there, up to the part after it
        // ([`Self::flow_top_line`]).
        let starts = blocks.starts();
        let drawn = self.review.top_line.clone();
        let line_for_confirmation = flowing.clone();
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
                .child(self.button("github-fixture-move",if self.review.fixture_moved{"Fixture head moved"}else{"Move fixture head"},"",false, cx).debug_selector(||"github-fixture-move".into()).disabled(pending||self.review.fixture_moved).on_click(cx.listener(|this,_,_,cx|{this.review.fixture_moved=true;this.confirm=None;this.notice=Some("Fixture head moved remotely. This view and its draft retain the original commit. Submit to verify stale-head refusal, or Refresh PRs and reopen to inspect the new head.".into());cx.notify();})))))
            .child(self.part("toolbar",div(),cx).flex().flex_wrap().items_center().gap_2()
                // Focus moved onto the field, by a click, by Name repository below, which
                // reveals it even when it holds focus already, or by the panel reopening onto
                // it, scrolls it into view with its ring, as a Tab onto it does, and only where
                // it is not whole already. It is a part of its own, as each control in the row
                // is: the row can be taller than the body, as it was at 18 pt in a 461 × 490
                // window while the body stopped short of the dialog's cap, and revealing the
                // whole row scrolled a field already whole, and the banner above it out of
                // view.
                .child(self.part("destination",div(),cx).debug_selector(||"github-destination".into()).w(appearance::ui_size(280.)).min_w(px(190.)).child(review::one_line()).child(review::starts_block()).child(div().on_children_prepainted(self.reveal_field_on_focus(self.destination.read(cx).focus_handle(cx))).child(self.stop_mark(&self.destination.read(cx).focus_handle(cx),review::ring_room,false,false)).child(Input::new(&self.destination).aria_label("GitHub destination owner and repository").disabled(pending))))
                .child(laid_out(self.part("refresh",div(),cx).child(review::starts_block()).child(self.button("github-refresh",if self.review.fixture{"Refresh fixture PRs"}else{"Refresh PRs"},"refresh",false, cx).debug_selector(||"github-refresh".into()).disabled(pending).on_click(cx.listener(|this,event,window,cx|if !this.held.borrow().refuses(event){this.refresh(1,window,cx)}))),refresh_laid))
                .child(self.part("create",div(),cx).child(review::starts_block()).child(self.closing(button("github-create","New pull request","plus",false),&self.review.create_focus,pending||self.review.fixture).debug_selector(||"github-create".into()).on_click(cx.listener(|this,_,window,cx|{this.review.closer=Some(this.review.create_focus.clone());this.create(window,cx)}))))
                .child(div().flex_1()).child(self.part("account",div(),cx).child(review::starts_block()).child(self.closing(button("github-account-options",self.account.as_ref().map(|account|format!("Account · {account}")).unwrap_or_else(||"Connect account".into()),"",self.review.connection_open),&self.review.account_focus,pending).debug_selector(||"github-account-options".into()).toggled(self.review.connection_open).on_click(cx.listener(|this,_,_,cx|{
                    this.review.closer=Some(this.review.account_focus.clone());
                    this.review.connection_open= !this.review.connection_open;
                    cx.notify();})))))
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
                .map(|col| {
                    #[cfg(target_os = "linux")]
                    {
                        col.when(self.plaintext_keyring_note && !self.review.fixture, |col| {
                            col.child(
                                div()
                                    .id("github-plaintext-keyring-note")
                                    .debug_selector(|| "github-plaintext-keyring-note".into())
                                    .flex()
                                    .flex_wrap()
                                    .items_center()
                                    .justify_between()
                                    .gap_2()
                                    .py_2()
                                    .px(text_inset())
                                    .rounded(px(6.))
                                    .bg(rgb(p.subtle))
                                    .child(
                                        label(
                                            "github-plaintext-keyring-note-text",
                                            LINUX_PLAINTEXT_KEYRING_NOTE,
                                        )
                                        .flex_1()
                                        .min_w_0()
                                        .text_color(rgb(p.muted)),
                                    )
                                    .child(
                                        self.part("plaintext-keyring-dismiss", div(), cx)
                                            .child(review::starts_block())
                                            .child(
                                                self.button(
                                                    "github-plaintext-keyring-dismiss",
                                                    "Dismiss",
                                                    "",
                                                    false,
                                                    cx,
                                                )
                                                .debug_selector(|| {
                                                    "github-plaintext-keyring-dismiss".into()
                                                })
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.plaintext_keyring_note = false;
                                                    if let Some(owner) = this.owner.upgrade() {
                                                        owner.update(cx, |app, cx| {
                                                            app.settings
                                                                .linux_plaintext_keyring_note_dismissed =
                                                                true;
                                                            app.save_preferences(window, cx);
                                                        });
                                                    }
                                                    window.focus(
                                                        &this.review.connection_focus,
                                                        cx,
                                                    );
                                                    cx.notify();
                                                })),
                                            ),
                                    ),
                            )
                        })
                    }
                    #[cfg(not(target_os = "linux"))]
                    {
                        col
                    }
                })
                .child(div().flex().flex_wrap().gap_2()
                    .child(self.part("connect",div(),cx).child(review::starts_block()).child(self.button("github-connect","Connect GitHub CLI account","",false, cx).debug_selector(||"github-connect".into()).disabled(pending||self.review.fixture).on_click(cx.listener(|this,_,window,cx|this.connect(window,cx)))))
                    .child(self.part("sign-in-help",div(),cx).child(review::starts_block()).child(self.held_button("github-sign-in-help",copy_button(copy_feedback::Mark::new("github-sign-in-help",SIGN_IN_COMMAND),"Copy sign-in command","",cx),cx).on_click(|_,window,cx|copy_feedback::copy(window,cx,Some(copy_feedback::Mark::new("github-sign-in-help",SIGN_IN_COMMAND)),"sign-in command",SIGN_IN_COMMAND.into()))))
                    .child(self.part("disconnect",div(),cx).child(review::starts_block()).child(self.button("github-disconnect","Disconnect GitTurtle","",false, cx).debug_selector(||"github-disconnect".into()).disabled(pending||self.review.fixture).on_click(cx.listener(|this,_,window,cx|this.disconnect(window,cx))))))))))
            .children(flowing_line)
            .when(!self.create && (self.review.show_list||self.pull.is_none()),|element|element
                // The list scrolls, so it clips its rows on both axes. Its full-width rows keep
                // their rings' room inside that clip: its sides give the room back through their
                // margin, so the rows' text keeps the panel's edge, and above the first row and
                // below the last the room is the list's own (a negative margin there collapsed
                // the panel's scrolling body to its padding).
                .child(laid_out(review::clipped_lines(crate::native_accessibility::clips_children(div().on_children_prepainted({let (revealed,scroll,blocks,pull_rest)=(self.review.pull_revealed.clone(),self.review.pull_scroll.clone(),self.review.pull_blocks.clone(),self.review.pull_scroll_rest.clone());let (landed,body,shown)=(created.is_some(),self.review.body_scroll.clone(),self.review.revealed_focus.clone());move|_,window,cx|{
                    pull_rest.set(scroll.offset().y);
                    // The focus its rows' reveals showed ([`Panel::pull_part`]).
                    review::record_shown(&revealed,&scroll,|context|context.contains(review::PULL),window,cx);blocks.finish();
                    // A creation's row, the first, which this frame shows with the notice above
                    // the list ([`Created`]): its own reveal, which follows focus and would move
                    // the list or the body a frame later, is made, in the list and in the body.
                    if landed{for (shown,view) in [(&revealed,&scroll),(&shown,&body)]{*shown.borrow_mut()=review::FocusObservation{focus:window.focused(cx),viewport:view.bounds(),rem_size:window.rem_size()};}}}})
                    .id("github-pr-list").debug_selector(||"github-pr-list".into()).track_scroll(&self.review.pull_scroll).on_scroll(crate::tags::chain_scroll_wheel(self.review.pull_scroll.clone(),self.review.pull_scroll_rest.clone())).flex_shrink_0().max_h(px(200.)+room*2.).px(room).mx(-room).py(room).overflow_y_scroll().flex().flex_col().gap_1()
                    .children(self.rows.iter().zip(&self.row_parts).filter(|(pull,_)|self.pr_query.matches(pull)).map(|(pull,(_,part))|{let pull=pull.clone();let selected=self.pull.as_ref().is_some_and(|p|p.number==pull.number);let number=pull.number;let name=format!("#{} · {}",pull.number,pull.title);let context=format!("{} · {} → {} · {}",if pull.draft{"Draft"}else{"Open"},pull.head.branch,pull.base.branch,pull.user.login);
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
                        let title=self.held_button(("github-pr",number),crate::button_frame(("github-pr",number),&name,"",false).with_variant(flat).selected(selected),cx);
                        let pressed=!pending&&self.pressed.get()==Some(number);let (down,up,out)=(self.pressed.clone(),self.pressed.clone(),self.pressed.clone());
                        div().id(("github-pr-card",pull.number)).debug_selector(move||format!("github-pr-card-{number}")).child(review::one_line()).flex_shrink_0().rounded(px(7.)).bg(rgb(if selected||pressed{p.selected}else{p.panel})).when(!pending&&!pressed,|row|row.hover(move|row|row.bg(rgb(hover))))
                            .when(!pending,|row|row.capture_any_mouse_down(move|event,window,_|if event.button==MouseButton::Left{down.set(Some(number));window.refresh();}).capture_any_mouse_up(move|_,window,_|if up.take().is_some(){window.refresh();}).on_mouse_up_out(MouseButton::Left,move|_,window,_|if out.take().is_some(){window.refresh();})).flex().flex_col()
                            .child(self.pull_part(part.clone(),div(),cx).child(title.key_context("GitTurtleGithubButton").debug_selector(move||format!("github-pr-{number}")).w_full().accessibility_label(name.clone()).tooltip(name.clone()).child(row_text(format!("github-pr-title-{number}"),name)).disabled(pending).on_click(cx.listener(move|this,event,window,cx|if !this.held.borrow().refuses(event){this.inspect(pull.clone(),window,cx)}))))
                            .child(native_accessibility::static_text(div().id(("github-pr-context",number)),context.clone()).flex().px(text_inset()).pb_1().text_size(appearance::ui_text(12.)).text_color(rgb(p.muted)).tooltip({let context=context.clone();move|window,cx|Tooltip::new(context.clone()).build(window,cx)}).when(!pending,|line|line.on_click(cx.listener(move|this,_,window,cx|this.inspect(open.clone(),window,cx)))).child(row_text(format!("github-pr-context-{number}"),context)))
                    }))
                    // One line with the next step: a repository with no GitHub remote names
                    // the destination first, and one with a destination reads its PRs with
                    // Refresh PRs, above. Opening the panel itself contacts nothing. Name
                    // repository moves to a name field, not a search, so it draws no glyph.
                    // The empty state, its line and action, is one line of the body: the body's
                    // reveals leave neither cut at its edge.
                    // Rows a creation's answer gave, with no page listed, are not the open pull
                    // requests: one line under them says so and names the step that loads those.
                    .when(self.created_only&&!self.rows.is_empty(),|element|element.child(phrases("github-list-partial",if self.rows.len()==1{"Only the new pull request is shown · Refresh PRs loads the open ones"}else{"Only the new pull requests are shown · Refresh PRs loads the open ones"}).flex_shrink_0().px(text_inset()).pb_1().text_color(rgb(p.muted)).children(appearance::text_probe(||"github-list-partial".into()))))
                    .when(self.rows.is_empty(),|element|element.child(self.part("pr-empty",div(),cx).py_5().px(text_inset()).child(div().child(review::one_line()).child(if self.local_loaded&&self.destination.read(cx).value().trim().is_empty(){
                        EmptyState::new("github-list-empty","No GitHub remote · Name the repository to load its pull requests").tooltip("Enter the destination as owner/repository or a GitHub.com remote URL. Opening this panel reads local settings and drafts only.")
                            .action(self.held_button("github-choose-destination",crate::empty_state::action("github-choose-destination","Name repository",""),cx).disabled(pending).on_click(cx.listener(|this,_,window,cx|{let focus=this.destination.read(cx).focus_handle(cx);window.focus(&focus,cx);
                                // The field may hold focus already, as it does once the panel opens:
                                // the next frame reveals it all the same.
                                *this.review.revealed_focus.borrow_mut()=Default::default();cx.notify();})))
                    }else{
                        EmptyState::new("github-list-empty","No pull requests shown · Refresh PRs loads the open ones").tooltip("Opening this panel reads local settings and drafts only.")
                    }.block(cx)))))),&self.review.pull_blocks).when(self.review.reopening.holds_row(),|list|list.child(review::list_mark(self.review.reopening.clone()))),created.as_ref().map(|(_,list)|list)))
                // A row that cannot hold its controls wraps them below, whole. A label after a
                // Button takes the panel's text inset, which the Button gives back, so beside it
                // nothing moves and on a line of its own the label starts at the panel's edge
                // for text.
                .child(self.part("pr-pager",div(),cx).flex().flex_wrap().items_center().gap_2().child(self.button("github-previous","Previous PR page","",false, cx).debug_selector(||"github-previous".into()).mr(-text_inset()).disabled(pending||self.page<=1).on_click(cx.listener(|this,_,window,cx|this.refresh(this.page-1,window,cx))))
                    .child(phrases("github-page",format!("Page {} · up to {} PRs",self.page,github::PAGE_SIZE)).min_w_0().ml(text_inset()).text_color(rgb(p.muted)))
                    .child(self.button("github-next","Next PR page","",false, cx).debug_selector(||"github-next".into()).disabled(pending||!self.has_next).on_click(cx.listener(|this,_,window,cx|this.refresh(this.page+1,window,cx))))
                    .child(self.button("github-pr-filter-toggle",if self.pr_filter_open{"Hide filters"}else{"Filters"},"",self.pr_filter_open, cx).debug_selector(||"github-pr-filter-toggle".into()).toggled(self.pr_filter_open).on_click(cx.listener(|this,_,_,cx|{this.pr_filter_open = !this.pr_filter_open;cx.notify();}))))
                .when(self.pr_filter_open,|element|element.child(self.part("pr-filters",div(),cx).flex().flex_col().gap_2().py_2().px(text_inset()).rounded(px(6.)).bg(rgb(p.subtle))
                    .child(div().flex().flex_wrap().items_center().gap_2()
                        .children(github::PullStateFilter::ALL.into_iter().map(|state|{let active=self.pr_query.state==state;self.button(("github-filter-state",state as usize),state.label(),"",active,cx).toggled(active).disabled(pending).on_click(cx.listener(move|this,_,window,cx|{if this.pr_query.state!=state{this.pr_query.state=state;if this.listed.is_some(){this.refresh(1,window,cx);}else{cx.notify();}}}))}))
                        .children(github::PullSort::ALL.into_iter().map(|sort|{let active=self.pr_query.sort==sort;self.button(("github-filter-sort",sort as usize),sort.label(),"",active,cx).toggled(active).disabled(pending).on_click(cx.listener(move|this,_,window,cx|{if this.pr_query.sort!=sort{this.pr_query.sort=sort;if this.listed.is_some(){this.refresh(1,window,cx);}else{cx.notify();}}}))}))
                        .child(self.button("github-filter-draft","Drafts only","",self.pr_query.draft_only,cx).toggled(self.pr_query.draft_only).on_click(cx.listener(|this,_,_,cx|{this.pr_query.draft_only = !this.pr_query.draft_only;cx.notify();}))))
                    .child(Input::new(&self.pr_search).aria_label("Filter pull requests by text")))))
            .when_some(self.pull.as_ref(),|element,pull|element
                // The PR's header and its section tabs, shown from the title's line as the PR
                // opens.
                .child(div().flex().flex_col().gap_2().on_children_prepainted(self.reveal_opened(title_line,cx)).child(self.part("pull-header",div(),cx).flex().flex_col().gap_2()
                    .child(div().flex().flex_wrap().items_center().gap_2().on_children_prepainted(move|children,_,_|{
                        // The title's line: the Button's where the title stands beside it, its
                        // own where it wraps below.
                        if let [back,title]=children[..]{line.set(if title.top()<back.bottom(){back.top().min(title.top())}else{title.top()});}
                    }).child(self.button("github-back-list",if self.review.show_list{"Hide PR list"}else{"Pull requests"},"arrow-left",false, cx).debug_selector(||"github-back-list".into()).mr(-text_inset()).disabled(pending).on_click(cx.listener(|this,_,window,cx|{this.save_draft(window,cx);this.review.show_list= !this.review.show_list;cx.notify();})))
                        .child(label("github-pull-title",format!("#{} · {}",pull.number,pull.title)).debug_selector(||"github-pull-title".into()).min_w_0().ml(text_inset()).font_weight(FontWeight::SEMIBOLD).text_size(appearance::ui_text(15.))))
                    .child(label("github-branch-context",format!("{} → {} · {}",pull.head.branch,pull.base.branch,if pull.merged_at.is_some(){"Merged PR"}else if pull.state.eq_ignore_ascii_case("closed"){"Closed PR"}else if pull.draft{"Draft PR"}else{"Open PR"})).ml(text_inset()).text_color(rgb(p.muted))))
                .child(self.render_tabs(cx)))
                .when(self.review.section==Section::Overview,|element|element
                    // The last section tab owns the PR's captured identities, and Tab onto it shows
                    // them with it where both fit the body; where they cannot, they are a group stop
                    // of their own, revealed from their top, their ring on their edge
                    // (`review::Owned`). Their edge is always drawn, clear at rest, and with their
                    // side padding makes the panel's text inset, so their text keeps the panel's
                    // text edge and becoming a stop moves nothing.
                    .child(div().on_children_prepainted(self.review.identities.lay_out_content(self.review.body_scroll.clone()))
                        .child(self.ringed_part("identities",div(),&self.review.identities_focus,cx)
                            .child(div().id("github-identities").role(Role::Group).aria_label("Captured identities: the base and head commits this review is pinned to").when(self.review.identities.apart()||self.review.identities_focus.is_focused(window),|block|block.track_focus(&self.review.identities_focus)).border_1().border_color(if self.ringed(&self.review.identities_focus,cx){rgb(p.accent).into()}else{gpui_kit::transparent_black()}).rounded(px(6.)).px(text_inset()-px(1.))
                                .child(label("github-captured-identities",format!("Before · {}\nAfter  · {}",pull.base.sha,pull.head.sha)).text_color(rgb(p.muted))))))
                    .when_some(self.description.as_ref(),|element,description|{
                        let has_rendered = self.description_view.is_some();
                        let show_rendered = has_rendered && !self.description_source;
                        let rendered_view = self.description_view.clone();
                        element.child(
                            self.part("description", div(), cx)
                                .flex()
                                .flex_col()
                                .gap_1()
                                .flex_shrink_0()
                                .when(has_rendered, |block| {
                                    block.child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_1()
                                            .child(
                                                self.button("github-desc-rendered", "Rendered", "", show_rendered, cx)
                                                    .toggled(show_rendered)
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.description_source = false;
                                                        cx.notify();
                                                    })),
                                            )
                                            .child(
                                                self.button("github-desc-source", "Source", "", !show_rendered, cx)
                                                    .toggled(!show_rendered)
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.description_source = true;
                                                        cx.notify();
                                                    })),
                                            ),
                                    )
                                })
                                .child(if let (true, Some(view)) = (show_rendered, rendered_view) {
                                    div()
                                        .h(px(220.))
                                        .rounded(px(6.))
                                        .border_1()
                                        .border_color(rgb(p.border))
                                        .overflow_hidden()
                                        .child(view)
                                        .into_any_element()
                                } else {
                                    div()
                                        .relative()
                                        .h(px(120.))
                                        .flex_shrink_0()
                                        .child(review::editor_rows(description))
                                        .child(
                                            crate::editor_find::Editor::new(description)
                                                .readonly(true)
                                                .h_full()
                                                .aria_label("Pull request description, exact Markdown source"),
                                        )
                                        .into_any_element()
                                }),
                        )
                    })
                    .when_some(self.status.as_ref(),|element,status|{
                        let passed = status.check_runs.iter().filter(|c| c.conclusion == github::CheckConclusion::Success).count();
                        let failed = status.check_runs.iter().filter(|c| c.conclusion == github::CheckConclusion::Failure).count();
                        let pending_runs = status.check_runs.iter().filter(|c| c.conclusion == github::CheckConclusion::Pending).count();
                        let has_runs = !status.check_runs.is_empty();
                        let runs_summary = format!(
                            "Checks · {} passed · {} failed · {} pending{}",
                            passed,
                            failed,
                            pending_runs,
                            if status.incomplete { " · more checks outside this preview" } else { "" }
                        );
                        element.child(
                            div()
                                .py_3()
                                .px(text_inset())
                                .bg(rgb(p.subtle))
                                .rounded(px(6.))
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(label(
                                    "github-review-status",
                                    format!(
                                        "Reviews · {}",
                                        if status.reviews.is_empty() {
                                            "None reported".into()
                                        } else {
                                            status.reviews.join(" · ")
                                        }
                                    ),
                                ))
                                .child(if has_runs {
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .child(
                                            div()
                                                .flex()
                                                .flex_wrap()
                                                .items_center()
                                                .gap_2()
                                                .child(label("github-check-status", runs_summary))
                                                .child(
                                                    self.button(
                                                        "github-checks-toggle",
                                                        if self.checks_expanded {
                                                            "Hide checks"
                                                        } else {
                                                            "Show checks"
                                                        },
                                                        "",
                                                        self.checks_expanded,
                                                        cx,
                                                    )
                                                    .toggled(self.checks_expanded)
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.checks_expanded = !this.checks_expanded;
                                                        cx.notify();
                                                    })),
                                                ),
                                        )
                                        .when(self.checks_expanded, |col| {
                                            col.children(status.check_runs.iter().enumerate().map(|(ix, run)| {
                                                let outcome_word = match run.conclusion {
                                                    github::CheckConclusion::Success => "Passed",
                                                    github::CheckConclusion::Failure => "Failed",
                                                    github::CheckConclusion::Pending => "Pending",
                                                    github::CheckConclusion::Neutral => "Neutral",
                                                    github::CheckConclusion::Skipped => "Skipped",
                                                };
                                                let clean_name = gitturtle_core::strip_bidi_controls(&run.name);
                                                let safe_url = run
                                                    .details_url
                                                    .as_ref()
                                                    .filter(|u| u.starts_with("https://"))
                                                    .cloned();
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap_2()
                                                    .text_size(appearance::ui_text(11.))
                                                    .child(
                                                        div()
                                                            .flex_1()
                                                            .min_w_0()
                                                            .truncate()
                                                            .text_color(rgb(p.muted))
                                                            .child(format!("{outcome_word} · {clean_name} ({})", run.status)),
                                                    )
                                                    .when_some(safe_url, |row, url| {
                                                        let tooltip_url = url.clone();
                                                        let accessible = format!("Open details for check {clean_name}");
                                                        row.child(
                                                            self.button(("github-check-details", ix), "Details", "", false, cx)
                                                                .accessibility_label(accessible)
                                                                .tooltip(tooltip_url)
                                                                .on_click(move |_, _, cx| cx.open_url(&url)),
                                                        )
                                                    })
                                            }))
                                        })
                                        .into_any_element()
                                } else {
                                    label(
                                        "github-check-status",
                                        format!(
                                            "Checks · {}{}",
                                            if status.checks.is_empty() {
                                                "None reported".into()
                                            } else {
                                                status.checks.join(" · ")
                                            },
                                            if status.incomplete {
                                                " · Additional status entries are outside this preview"
                                            } else {
                                                ""
                                            }
                                        ),
                                    )
                                    .into_any_element()
                                }),
                        )
                    })
                    .child(self.render_threads(None,window,cx))
                    .child({
                        let panel_more = cx.entity().downgrade();
                        let pull_is_draft = pull.draft;
                        let pull_is_open = pull.state.eq_ignore_ascii_case("open");
                        let pull_is_merged = pull.merged_at.is_some();
                        let has_node_id = pull.node_id.is_some();
                        let existing_labels = pull.labels.iter().map(|l| l.name.clone()).collect::<Vec<_>>().join(", ");
                        let existing_reviewers = pull.requested_reviewers.iter().map(|r| r.login.clone()).collect::<Vec<_>>().join(", ");
                        self.part("open-changes",div(),cx).flex().flex_wrap().items_center().gap_2()
                            .child(self.button("github-open-changes","Open captured local comparison","",false, cx).debug_selector(||"github-open-changes".into()).disabled(pending).on_click(cx.listener(|this,_,window,cx|this.changes(window,cx))))
                            .child(self.button("github-checkout-pr","Checkout PR branch","",false, cx).debug_selector(||"github-checkout-pr".into()).disabled(pending||self.review.fixture).on_click(cx.listener(|this,_,window,cx|this.checkout_pr_branch(window,cx))))
                            .child(self.button("github-merge-pr","Merge…","",false, cx).debug_selector(||"github-merge-pr".into()).disabled(pending||self.review.fixture||pull_is_draft||!pull_is_open).on_click(cx.listener(|this,_,window,cx|this.open_merge_pr_dialog(window,cx))))
                            .child(self.button("github-open-browser","Open in browser","",false, cx).debug_selector(||"github-open-browser".into()).disabled(self.review.fixture).on_click(cx.listener(|this,_,_,cx|this.open_pr_in_browser(cx))))
                            .child(
                                self.button("github-pr-more","⋯","",false, cx)
                                    .debug_selector(||"github-pr-more".into())
                                    .tooltip("More PR actions (Draft, Close/Reopen, Labels, Reviewers)")
                                    .accessibility_label("More pull request actions")
                                    .disabled(pending||self.review.fixture)
                                    .app_menu(move |menu, _, _| {
                                        use gpui_kit::component::menu::PopupMenuItem;
                                        let p1 = panel_more.clone();
                                        let p2 = panel_more.clone();
                                        let p3 = panel_more.clone();
                                        let p4 = panel_more.clone();
                                        let labels_init = existing_labels.clone();
                                        let reviewers_init = existing_reviewers.clone();
                                        menu.item(
                                            PopupMenuItem::new(if pull_is_draft {
                                                "Mark ready for review…"
                                            } else {
                                                "Convert to draft…"
                                            })
                                            .disabled(!pull_is_open || !has_node_id)
                                            .on_click(move |_, window, cx| {
                                                let _ = p1.update(cx, |this, cx| {
                                                    let Some(node_id) = this
                                                        .pull
                                                        .as_ref()
                                                        .and_then(|p| p.node_id.clone())
                                                    else {
                                                        return;
                                                    };
                                                    let current_draft = this
                                                        .pull
                                                        .as_ref()
                                                        .is_some_and(|p| p.draft);
                                                    if let Ok(captured) = this.captured_pull(cx) {
                                                        this.queue_pr_action(
                                                            Action::SetPullDraft {
                                                                pull: captured,
                                                                node_id,
                                                                draft: !current_draft,
                                                            },
                                                            window,
                                                            cx,
                                                        );
                                                    }
                                                });
                                            }),
                                        )
                                        .item(
                                            PopupMenuItem::new(if pull_is_open {
                                                "Close pull request…"
                                            } else {
                                                "Reopen pull request…"
                                            })
                                            .disabled(!pull_is_open && pull_is_merged)
                                            .on_click(move |_, window, cx| {
                                                let _ = p2.update(cx, |this, cx| {
                                                    if let Ok(captured) = this.captured_pull(cx) {
                                                        this.queue_pr_action(
                                                            Action::SetPullState {
                                                                pull: captured,
                                                                open: !pull_is_open,
                                                            },
                                                            window,
                                                            cx,
                                                        );
                                                    }
                                                });
                                            }),
                                        )
                                        .separator()
                                        .item(
                                            PopupMenuItem::new("Edit labels…").on_click(
                                                move |_, window, cx| {
                                                    let labels_init = labels_init.clone();
                                                    let _ = p3.update(cx, |this, cx| {
                                                        this.open_pr_csv_dialog(
                                                            "Edit pull request labels",
                                                            "Enter comma-separated label names to replace the PR's labels.",
                                                            "bug, performance, needs-review",
                                                            labels_init,
                                                            true,
                                                            |pull, labels| Action::UpdateLabels {
                                                                pull,
                                                                labels,
                                                            },
                                                            window,
                                                            cx,
                                                        );
                                                    });
                                                },
                                            ),
                                        )
                                        .item(
                                            PopupMenuItem::new("Request reviewers…").on_click(
                                                move |_, window, cx| {
                                                    let reviewers_init = reviewers_init.clone();
                                                    let _ = p4.update(cx, |this, cx| {
                                                        this.open_pr_csv_dialog(
                                                            "Request pull request reviewers",
                                                            "Enter comma-separated GitHub usernames to request review from.",
                                                            "alice, bob",
                                                            reviewers_init,
                                                            false,
                                                            |pull, reviewers| {
                                                                Action::RequestReviewers {
                                                                    pull,
                                                                    reviewers,
                                                                }
                                                            },
                                                            window,
                                                            cx,
                                                        );
                                                    });
                                                },
                                            ),
                                        )
                                    }),
                            )
                    }))
                .when(self.review.section==Section::Files,|element|element.child(self.render_files(window,cx)))
                .when(self.review.section==Section::Review,|element|element.child(self.render_collected(cx))))
            .when(self.create,|element|element
                .child(self.button("github-create-back","Pull requests","arrow-left",false, cx).debug_selector(||"github-create-back".into()).disabled(pending).on_click(cx.listener(|this,_,window,cx|{this.save_draft(window,cx);this.leave_form(cx);this.confirm=None;this.review.show_list=true;this.focus_visible_section(window,cx);cx.notify();})))
                .child(label("github-create-fields","New pull request · published head → destination base").font_weight(FontWeight::SEMIBOLD))
                // Each one-line field is one line of the body, which its reveals leave whole.
                .child(div().debug_selector(||"github-create-title".into()).child(self.stop_mark(&self.title.read(cx).focus_handle(cx),review::ring_room,false,false)).child(review::one_line()).child(Input::new(&self.title).aria_label("Pull request title").disabled(pending)))
                .child(div().flex().gap_2().child(div().flex_1().child(review::one_line()).child(Input::new(&self.head).aria_label("Published head branch").disabled(pending))).child(div().flex_1().child(review::one_line()).child(Input::new(&self.base).aria_label("Destination base branch").disabled(pending))))
                .child(div().flex().gap_2().children([(true,"Draft"),(false,"Ready")].map(|(draft,name)|self.button(name,name,"",self.draft==draft, cx).toggled(self.draft==draft).disabled(pending).on_click(cx.listener(move|this,_,window,cx|{this.draft=draft;this.confirm=None;this.save_draft(window,cx);}))))
                    .child(self.button("github-load-templates","Load local base templates","",false, cx).disabled(pending).on_click(cx.listener(|this,_,window,cx|this.load_templates(window,cx)))))
                .children(self.templates.iter().enumerate().map(|(index,template)|{let template=template.clone();self.button(("github-template",index),format!("Append {} · {}",template.path,&template.revision[..8]),"",false, cx).disabled(pending).on_click(cx.listener(move|this,_,window,cx|{let body=format!("{}{}{}",this.body.read(cx).value(),if this.body.read(cx).value().is_empty(){""}else{"\n\n"},template.body);this.body.update(cx,|input,cx|input.set_value(body,window,cx));this.save_draft(window,cx);} ))})))
            .when(show_editor,|element|element
                .when(!self.create,|element|element.child(self.part("events",div(),cx).flex().flex_wrap().gap_1()
                    .children([ReviewEvent::Comment,ReviewEvent::Approve,ReviewEvent::RequestChanges].map(|event|self.button(event.label(),event.label(),"",!self.discussion&&self.event==event, cx).toggled(!self.discussion&&self.event==event).disabled(pending).on_click(cx.listener(move|this,_,window,cx|{this.discussion=false;this.event=event;this.confirm=None;this.save_draft(window,cx);}))))
                    .child(self.button("github-discussion","Discussion only","",self.discussion, cx).toggled(self.discussion).disabled(pending||!self.review.comments.is_empty()||self.review.composing.is_some()).on_click(cx.listener(|this,_,_,cx|{this.discussion=true;this.confirm=None;cx.notify();})))))
                .child(div().debug_selector(||"github-review-summary".into()).on_children_prepainted(self.reveal_field_on_focus(self.body.read(cx).focus_handle(cx))).child(self.stop_mark(&self.body.read(cx).focus_handle(cx),review::ring_room,false,false)).child(review::editor_rows(&self.body)).child(Textarea::new(&self.body).h(appearance::ui_size(130.)).aria_label(if self.create{"Pull request description"}else{"Review summary"}).disabled(pending)))
                // The draft status beside Review submission… where it fits there whole, at the
                // panel's text inset; otherwise on a line of its own below it, wrapping, so its
                // remedy stays readable and the button stays inside the panel (at 18 pt a failed
                // save's status pushed the button past the panel's right edge).
                .child(self.part("submit",div(),cx).flex().flex_wrap_reverse().items_center().gap_2().child(label("github-draft-status",self.save_status.clone()).debug_selector(||"github-draft-status".into()).min_w_0().mx(text_inset()).role(Role::Status).polite().text_color(rgb(p.muted)))
                    .child(self.button("github-review-submit","Review submission…","",true, cx).debug_selector(||"github-review-submit".into()).ml_auto().disabled(pending).on_click(cx.listener(|this,_,window,cx|this.review_submission(window,cx))))))
            .child(self.render_reply_composer(cx))
            // The outbound confirmation, its stop a region that scrolls its destination and
            // text ([`Panel::render_confirmation`]).
            .when_some(self.confirm.as_ref(),|element,confirmation|element.child(self.render_confirmation(confirmation,line_for_confirmation,window,cx)))
            .child(self.part("recovery",div(),cx).flex().flex_wrap().items_center().gap_2().child(self.button("github-recovery-toggle",format!("Recover drafts · {}",self.saved.iter().filter(|draft| is_recoverable_draft(draft)).count()),"",self.review.recovery_open, cx).debug_selector(||"github-recovery-toggle".into()).mr(-text_inset()).toggled(self.review.recovery_open).on_click(cx.listener(|this,_,_,cx|{this.review.recovery_open= !this.review.recovery_open;cx.notify();})))
                .child(phrases("github-passive-policy",if cfg!(target_os="macos"){"Drafts stay on this Mac · outbound actions are explicit"}else{"Drafts stay on this computer · outbound actions are explicit"}).min_w_0().ml(text_inset()).text_color(rgb(p.muted))))
            .when(self.review.recovery_open,|element|element
                .child(label("github-saved-heading","Captured drafts · Copy includes all inline text and original positions; older heads are never remapped").ml(text_inset()).text_color(rgb(p.muted)))
                .child(review::clipped(crate::native_accessibility::clips_children(div().id("github-saved-drafts").debug_selector(||"github-saved-drafts".into()).track_scroll(&self.review.saved_drafts_scroll.handle).on_scroll(self.review.saved_drafts_scroll.wheel()).flex_shrink_0().max_h(px(160.)).overflow_y_scroll().flex().flex_col().gap_2().children(self.saved.iter().filter(|draft| is_recoverable_draft(draft)).take(128).enumerate().map(|(index,draft)|{let draft=draft.clone();let discard=draft.clone();let mark=copy_feedback::Mark::new(("github-copy-draft",index),draft.key());let copy_mark=mark.clone();div().flex_shrink_0().flex().flex_col().gap_1().child(label(("github-draft-identity",index),match &draft {Draft::Reply(reply)=>format!("Reply · {} #{} · {} · {}",reply.target.pull.repository.label(),reply.target.pull.number,reply.target.path,reply.target.account),_=>draft.key()}).ml(text_inset()).text_color(rgb(p.muted)))
                    .child(div().flex().gap_2().child(self.held_button(("github-copy-draft",index),copy_button(mark,"Copy complete draft", "", cx),cx).on_click(move|_,window,cx|copy_feedback::copy(window,cx,Some(copy_mark.clone()),"draft",draft.recovery_text())))
                        .child(self.button(("github-discard-draft",index),"Discard saved draft","",false, cx).disabled(pending).on_click(cx.listener(move|this,_,window,cx|this.discard_draft(discard.clone(),window,cx)))))}))).on_prepaint(self.review.saved_drafts_scroll.observe())))
                .when(!self.attempts.is_empty(),|element|element.child(label("github-attempt-heading","Previous outbound attempts · inspect the destination before repeating").ml(text_inset()))
                    .child(review::clipped(crate::native_accessibility::clips_children(div().id("github-attempt-list").debug_selector(||"github-attempt-list".into()).track_scroll(&self.review.attempt_list_scroll.handle).on_scroll(self.review.attempt_list_scroll.wheel()).flex_shrink_0().max_h(px(100.)).overflow_y_scroll().flex().flex_col().gap_1().children(self.attempts.iter().rev().take(10).enumerate().map(|(index,attempt)|label(("github-attempt",index),format!("{}: {}",attempt.destination,attempt.outcome)).ml(text_inset()).flex_shrink_0())).on_prepaint(self.review.attempt_list_scroll.observe()))))));
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
        // Where reopening put focus, settled once the body's parts have found it.
        let (reopening, panel) = (self.review.reopening.clone(), cx.entity().downgrade());
        let (pull_revealed, list_revealed) = (
            self.review.pull_revealed.clone(),
            self.conversations.list_revealed.clone(),
        );
        #[cfg(test)]
        let list_scroll = self.conversations.list_scroll.clone();
        #[cfg(test)]
        let pull_scroll = self.review.pull_scroll.clone();
        // In the frame that first draws the reopened panel, the part holding the control focus
        // went back to is revealed from that frame's layout ([`review::Reopened`]).
        let content = review::reopened(
            content,
            self.review.reopening.clone(),
            self.review.body_scroll.clone(),
            self.review.body_blocks.clone(),
            self.review.revealed_focus.clone(),
            self.review.pull_scroll.clone(),
            self.review.pull_blocks.clone(),
        );
        // In the frame that first draws a creation's row, the body shows it with its notice
        // before the content prepaints ([`Created`]).
        let content = match created {
            Some((line, list)) => Created {
                content: content.into_any_element(),
                body: self.review.body_scroll.clone(),
                line,
                list,
            }
            .into_any_element(),
            None => content.into_any_element(),
        };
        // In the frame a failure lands in, the body shows its line before the content
        // prepaints ([`Failed`]).
        let content = match landed {
            Some((line, remedy)) => Failed {
                content,
                body: self.review.body_scroll.clone(),
                blocks: self.review.body_blocks.clone(),
                revealed: self.review.revealed_focus.clone(),
                line,
                remedy,
            }
            .into_any_element(),
            None => content,
        };
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
                    #[cfg(test)]
                    review::record_body_frame(&scroll, &list_scroll, &pull_scroll, window, cx);
                    let panel = panel.clone();
                    reopening.settle(
                        &scroll,
                        [&*observed, &*pull_revealed, &*list_revealed],
                        move |window, cx| {
                            let _ = panel.update(cx, |panel, cx| {
                                // With no PR open or new PR started, focus falls to the
                                // destination field, the body's first part, shown from the top
                                // whatever offset the body closed at, as a reopen that never
                                // held a control shows it. The field needed no reveal where the
                                // body closed, so the body stayed there, the fixture's banner
                                // above it out of view.
                                if panel.pull.is_none() && !panel.create {
                                    panel.review.body_scroll.set_offset(point(px(0.), px(0.)));
                                }
                                panel.focus_visible_section(window, cx);
                                cx.notify();
                            });
                        },
                        window,
                        cx,
                    );
                }))
                // An Enter coming up anywhere in the body ends a Return held off a review
                // comment ([`Panel::return_came_up`]).
                .capture_key_up(cx.listener(Self::return_came_up))
                // Return and Space coming up anywhere in the panel ([`HeldKeys`]): here, or
                // around the line above the body and the body.
                .when(top_line.is_none(), |body| {
                    body.capture_key_up(cx.listener(Self::note_key_up))
                })
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
                // The top line's controls too; a key-up in the body reaches both listeners,
                // whose ends are one.
                .capture_key_up(cx.listener(Self::return_came_up))
                .capture_key_up(cx.listener(Self::note_key_up))
                .key_context(HELD)
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
#[path = "github_view_tests.rs"]
mod tests;
