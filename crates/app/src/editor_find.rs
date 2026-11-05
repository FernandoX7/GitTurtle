//! Accessible Find presentation over the editor's public search engine.
//! Search never replaces the source entity or its selection, and owns a separate
//! decoration collection so patch colors survive closing or updating Find.

use crate::appearance::{editor_highlights, palette};
use gpui_kit::{
    AnyElement, App, AppContext, Context, DefiniteLength, Entity, EntityId, FocusHandle, Focusable,
    Global, HighlightStyle, InteractiveElement, IntoElement, ParentElement, Pixels, Render,
    RenderOnce, SharedString, StyleRefinement, Styled, Subscription, UnderlineStyle, WeakEntity,
    Window,
    base::ElementExt,
    component::{
        Disableable, Sizable, Theme,
        checkbox::Checkbox,
        input::{
            Editor as NativeEditor, EditorState, Enter, Escape, Input, InputEvent, InputState,
            OutdentInline, Replace, Search, TextDecoration, TextDecorationCollection,
        },
    },
    div, px, relative, rgb,
};
use std::{
    cell::RefCell,
    mem::size_of,
    ops::Range,
    rc::{Rc, Weak},
};

const RETAINED_PANELS: usize = 64;
const MAX_QUERY_BYTES: usize = 4096;
const MAX_HIGHLIGHTS: usize = 4096;

#[derive(Default)]
struct Panels(Vec<Entity<FindBar>>);
impl Global for Panels {}

struct ReservedLayer {
    editor: WeakEntity<EditorState>,
    /// Find's collection, made before any other on the editor so its current match's underline
    /// hides a patch's changed-word rule beneath it; `None` past [`RETAINED_PANELS`] reserved
    /// collections, where Find draws its matches on a collection of its own made after the
    /// patch's, underlined and without a background ([`FindBar::background_available`]).
    decorations: Option<TextDecorationCollection>,
    patch: Option<Weak<RefCell<PatchLayer>>>,
}
#[derive(Default)]
struct ReservedLayers(Vec<ReservedLayer>);
impl Global for ReservedLayers {}

/// Reserve a Find collection without constructing its input or panel. The
/// pinned renderer composes overlapping properties in hash order, so patch
/// backgrounds are explicitly removed underneath Find instead of relying on
/// collection order. Handles retain neither released editors nor patch data.
/// Past [`RETAINED_PANELS`] reserved collections the editor is still listed,
/// without one, so Find can mask its patch layer there too (`PatchLayer::underlined`).
pub fn reserve_highlight_layer(editor: &Entity<EditorState>, cx: &mut App) {
    if cx.try_global::<ReservedLayers>().is_some_and(|layers| {
        layers
            .0
            .iter()
            .any(|layer| layer.editor.entity_id() == editor.entity_id())
    }) {
        return;
    }
    let mut layers = std::mem::take(&mut cx.default_global::<ReservedLayers>().0);
    layers.retain(|layer| layer.editor.upgrade().is_some());
    // The reserved collections stay bounded at `RETAINED_PANELS` if a future
    // caller retains more groups; Find's fallback still has a visible
    // underline without competing backgrounds. The list itself is not bounded
    // there: it holds one entry per live editor that reserved, each of weak
    // handles alone, an editor past the bound listed without a collection so
    // Find can mask its patch layer, and released editors are dropped above.
    let reserved = layers
        .iter()
        .filter(|layer| layer.decorations.is_some())
        .count();
    let decorations = (reserved < RETAINED_PANELS).then(|| {
        editor.update(cx, |source, cx| {
            source.create_decorations_collection(Vec::new(), cx)
        })
    });
    layers.push(ReservedLayer {
        editor: editor.downgrade(),
        decorations,
        patch: None,
    });
    cx.default_global::<ReservedLayers>().0 = layers;
}

struct PatchLayer {
    collection: TextDecorationCollection,
    source: Vec<TextDecoration>,
    matches: Vec<Range<usize>>,
    /// Whether Find underlines every one of `matches` and draws no background under them, as
    /// it does on an editor without a reserved collection: then the patch keeps its
    /// backgrounds and gives up its changed-word rules under the matches, since an underline
    /// on Find's later collection does not hide them and two strokes would run one above the
    /// other under every match (decision, 2026-11-05: the rule gives way under an underline).
    underlined: bool,
}

impl PatchLayer {
    /// The decorations the editor draws for this layer under Find's matches.
    fn visible(&self) -> Vec<TextDecoration> {
        if self.underlined {
            without_match_rules(&self.source, &self.matches)
        } else {
            without_match_backgrounds(&self.source, &self.matches)
        }
    }
}

/// Patch styles owned with their editor view; Find only holds a weak handle.
#[derive(Clone)]
pub struct PatchDecorations(Rc<RefCell<PatchLayer>>);

impl PatchDecorations {
    /// Decorations this layer keeps and the editor's copy of them, which splits a decoration
    /// around each Find match (at most two more per match), plus the match ranges.
    pub fn retained_bytes(&self) -> usize {
        let layer = self.0.borrow();
        (2 * layer.source.len() + 2 * layer.matches.len()) * size_of::<TextDecoration>()
            + layer.matches.len() * size_of::<Range<usize>>()
    }
    pub fn set(&self, decorations: Vec<TextDecoration>, cx: &mut App) {
        let (collection, visible) = {
            let mut layer = self.0.borrow_mut();
            layer.source = decorations;
            (layer.collection.clone(), layer.visible())
        };
        collection.set(visible, cx);
    }
}

pub fn patch_decorations(
    editor: &Entity<EditorState>,
    decorations: Vec<TextDecoration>,
    cx: &mut App,
) -> PatchDecorations {
    reserve_highlight_layer(editor, cx);
    let collection = editor.update(cx, |source, cx| {
        source.create_decorations_collection(Vec::new(), cx)
    });
    let patch = PatchDecorations(Rc::new(RefCell::new(PatchLayer {
        collection,
        source: Vec::new(),
        matches: Vec::new(),
        underlined: false,
    })));
    if let Some(layer) = cx
        .default_global::<ReservedLayers>()
        .0
        .iter_mut()
        .find(|layer| layer.editor.entity_id() == editor.entity_id())
    {
        layer.patch = Some(Rc::downgrade(&patch.0));
    }
    // Find may already paint matches on this editor, as when a patch's decorations arrive
    // after its plain text: hand them to the new layer so no tint stays under them.
    if let Some(bar) = panel(editor, cx) {
        bar.update(cx, |bar, cx| {
            bar.painted = None;
            bar.sync_highlights(cx);
        });
    }
    patch.set(decorations, cx);
    patch
}

/// The patch layer Find masks on this editor, while one is attached and its handle alive.
#[cfg(test)]
pub fn patch_layer(editor: &Entity<EditorState>, cx: &App) -> Option<PatchDecorations> {
    cx.try_global::<ReservedLayers>()?
        .0
        .iter()
        .find(|layer| layer.editor.entity_id() == editor.entity_id())?
        .patch
        .as_ref()?
        .upgrade()
        .map(PatchDecorations)
}

#[cfg(test)]
impl PatchDecorations {
    /// The decorations this layer has the editor draw, after checking that the editor holds
    /// exactly their ranges in this layer's collection.
    pub fn drawn(&self, cx: &App) -> Vec<TextDecoration> {
        let layer = self.0.borrow();
        let visible = layer.visible();
        assert_eq!(
            layer.collection.get_ranges(cx),
            visible
                .iter()
                .map(|decoration| decoration.range.clone())
                .collect::<Vec<_>>(),
            "the editor holds the ranges its patch layer set"
        );
        visible
    }
}

/// Mask `editor`'s patch layer under Find's match `ranges`, which Find draws `underlined`, each
/// with an underline and no background, where the editor has no reserved collection.
fn set_match_ranges(
    editor: &WeakEntity<EditorState>,
    ranges: Vec<Range<usize>>,
    underlined: bool,
    cx: &mut App,
) {
    let patch = cx
        .try_global::<ReservedLayers>()
        .and_then(|layers| {
            layers
                .0
                .iter()
                .find(|layer| layer.editor.entity_id() == editor.entity_id())
        })
        .and_then(|layer| layer.patch.as_ref())
        .and_then(Weak::upgrade);
    if let Some(patch) = patch {
        let (collection, visible) = {
            let mut layer = patch.borrow_mut();
            if layer.matches == ranges && layer.underlined == underlined {
                return;
            }
            layer.matches = ranges;
            layer.underlined = underlined;
            (layer.collection.clone(), layer.visible())
        };
        collection.set(visible, cx);
    }
}

// Both inputs are ordered, disjoint UTF-8 ranges prepared by the patch worker
// and search engine. Visit the metadata once; never rescan source text. Keep
// foreground/font styles so Find changes neither syntax nor diff text colors.
fn without_match_backgrounds(
    source: &[TextDecoration],
    matches: &[Range<usize>],
) -> Vec<TextDecoration> {
    split_under_matches(
        source,
        matches,
        |decoration| decoration.style.background_color.is_some(),
        |decoration| decoration.style.background_color = None,
    )
}

/// `source` with the bottom rule of each decoration that draws one taken away under `matches`,
/// every other property and decoration kept whole, for Find's underlined matches
/// ([`PatchLayer::underlined`]).
fn without_match_rules(source: &[TextDecoration], matches: &[Range<usize>]) -> Vec<TextDecoration> {
    split_under_matches(
        source,
        matches,
        |decoration| decoration.bottom_rule.is_some(),
        |decoration| decoration.bottom_rule = None,
    )
}

/// `source` with each decoration `masked` selects split around `matches`, `unmask` applied to
/// its pieces under them, and every other decoration kept whole.
fn split_under_matches(
    source: &[TextDecoration],
    matches: &[Range<usize>],
    masked: impl Fn(&TextDecoration) -> bool,
    unmask: impl Fn(&mut TextDecoration),
) -> Vec<TextDecoration> {
    let piece = |decoration: &TextDecoration, range: Range<usize>| TextDecoration {
        range,
        ..decoration.clone()
    };
    let mut output = Vec::with_capacity(source.len().saturating_add(matches.len() * 2));
    let mut first_match = 0;
    for decoration in source {
        if !masked(decoration) {
            output.push(decoration.clone());
            continue;
        }
        while first_match < matches.len() && matches[first_match].end <= decoration.range.start {
            first_match += 1;
        }
        let mut cursor = decoration.range.start;
        for range in &matches[first_match..] {
            if range.start >= decoration.range.end {
                break;
            }
            let start = range.start.max(cursor);
            let end = range.end.min(decoration.range.end);
            if cursor < start {
                output.push(piece(decoration, cursor..start));
            }
            if start < end {
                let mut unmasked = piece(decoration, start..end);
                unmask(&mut unmasked);
                output.push(unmasked);
                cursor = end;
            }
        }
        if cursor < decoration.range.end {
            output.push(piece(decoration, cursor..decoration.range.end));
        }
    }
    output
}

fn reserved_layer(editor: &Entity<EditorState>, cx: &App) -> Option<TextDecorationCollection> {
    cx.try_global::<ReservedLayers>()?
        .0
        .iter()
        .find(|layer| layer.editor.entity_id() == editor.entity_id())
        .and_then(|layer| layer.decorations.clone())
}

fn panel(editor: &Entity<EditorState>, cx: &App) -> Option<Entity<FindBar>> {
    cx.try_global::<Panels>()?
        .0
        .iter()
        .find(|panel| panel.read(cx).editor.entity_id() == editor.entity_id())
        .cloned()
}

/// The open find bar's height as it last prepainted, for a view that fits
/// whole lines below it (the GitHub review's source).
pub fn panel_height(editor: &Entity<EditorState>, cx: &App) -> Pixels {
    panel(editor, cx).map_or(px(0.), |panel| {
        let panel = panel.read(cx);
        if panel.open { panel.height } else { px(0.) }
    })
}

/// A source editor retaining the native editor's styling and focus behavior.
#[derive(IntoElement)]
pub struct Editor {
    state: Entity<EditorState>,
    native: NativeEditor,
    height: DefiniteLength,
    label: SharedString,
    /// What stands beside the text under Find's bar, and the room it takes
    /// ([`Self::beside`]).
    beside: Option<(Pixels, Vec<AnyElement>)>,
    /// The editor this one shares one Find bar with, which their owner
    /// draws across both, and the side this one is
    /// ([`Self::sharing_find_with`]).
    partner: Option<(Entity<EditorState>, Side)>,
}

/// Which of two editors that share one Find bar
/// ([`Editor::sharing_find_with`]) an editor is: Split's Before, the left
/// side, or After, the right. The bar's empty field names it, and the bar
/// marks its half.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Before,
    After,
}

thread_local! {
    /// The placeholders the shared bar's field last fitted on each side.
    static BEFORE_FIT: crate::workspace::PlaceholderFit = const { RefCell::new(None) };
    static AFTER_FIT: crate::workspace::PlaceholderFit = const { RefCell::new(None) };
}

impl Side {
    /// What the bar's empty field says while it searches this side, from
    /// its longest form: the side's name stays whole where "Find in" does
    /// not fit beside it.
    fn placeholders(self) -> &'static [&'static str] {
        match self {
            Self::Before => &["Find in Before", "Before"],
            Self::After => &["Find in After", "After"],
        }
    }

    fn fit(self) -> &'static std::thread::LocalKey<crate::workspace::PlaceholderFit> {
        match self {
            Self::Before => &BEFORE_FIT,
            Self::After => &AFTER_FIT,
        }
    }
}

impl Editor {
    pub fn new(state: &Entity<EditorState>) -> Self {
        Self {
            state: state.clone(),
            native: NativeEditor::new(state).text_size(crate::appearance::code_text()),
            height: relative(1.),
            label: "File text".into(),
            beside: None,
            partner: None,
        }
    }
    /// Draws `elements` beside the text, `width` across before it, under
    /// Find's bar, which spans them as it spans the text: a patch's line
    /// numbers and partial actions, positioned absolutely in the text's
    /// region, whose top is the bar's foot.
    pub fn beside(mut self, width: Pixels, elements: Vec<AnyElement>) -> Self {
        self.beside = Some((width, elements));
        self
    }
    /// Shares one Find bar with `partner`, as Split's sides do, this editor
    /// being `side`: their owner draws the bar of either across both
    /// ([`shared_bar`]), so this editor draws none above its text. Opening
    /// Find here moves the partner's open bar here, with its query and Match
    /// case, so the two never stack; Escape in either's text closes it. The
    /// bar's empty field names the side it searches, and the bar marks that
    /// side's half of its foot in the accent colour.
    pub fn sharing_find_with(mut self, partner: &Entity<EditorState>, side: Side) -> Self {
        self.partner = Some((partner.clone(), side));
        self
    }
    /// The kit editor this wrapper renders. Code keeps its characters as
    /// typed in every family: unless the caller refined the font features,
    /// the editor gets the code features of the family it draws in, which is
    /// the theme's code family unless the caller refined another.
    fn native(mut self, cx: &App) -> NativeEditor {
        let text = &self.native.style().text;
        if text.font_features.is_some() {
            return self.native;
        }
        let family = text.font_family.clone();
        let family = family.unwrap_or_else(|| Theme::global(cx).mono_font_family.clone());
        self.native
            .font_features(crate::appearance::code_font_features_for(&family))
    }
    pub fn h(mut self, height: impl Into<DefiniteLength>) -> Self {
        self.height = height.into();
        self
    }
    pub fn readonly(mut self, value: bool) -> Self {
        self.native = self.native.readonly(value);
        self
    }
    pub fn bordered(mut self, value: bool) -> Self {
        self.native = self.native.bordered(value);
        self
    }
    pub fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self.native = self.native.aria_label(self.label.clone());
        self
    }
}
impl Styled for Editor {
    fn style(&mut self) -> &mut StyleRefinement {
        self.native.style()
    }
}
impl RenderOnce for Editor {
    fn render(mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (source, label, height) = (self.state.clone(), self.label.clone(), self.height);
        let (beside, partner) = (self.beside.take(), self.partner.take());
        let side = partner.as_ref().map(|(_, side)| *side);
        let native = self.native(cx);
        let owner = window.current_view();
        let existing = panel(&source, cx);
        if let Some(panel) = &existing {
            panel.update(cx, |bar, _| {
                bar.owner = owner;
                bar.label = label.clone();
            });
        }
        let state = source.downgrade();
        let replacement = state.clone();
        let replacement_label = label.clone();
        let shared = partner.as_ref().map(|(partner, _)| partner.downgrade());
        let replacement_shared = shared.clone();
        // Escape in the text closes its own bar, or the bar it shares with
        // its partner, which the partner opened.
        let closing = [
            existing.as_ref().map(Entity::downgrade),
            partner
                .as_ref()
                .and_then(|(partner, _)| panel(partner, cx))
                .map(|bar| bar.downgrade()),
        ];
        // Shift+Tab from the text returns to the bar searching it, whose
        // last control comes before the text in Tab order.
        let returning = existing.as_ref().map(Entity::downgrade);
        // A shared bar is drawn by the editors' owner ([`shared_bar`]).
        let own_bar = existing.filter(|bar| partner.is_none() && bar.read(cx).open);
        div()
            .w_full()
            .h(height)
            .min_h_0()
            .flex()
            .flex_col()
            .capture_action(move |_: &Search, window, cx| {
                if let Some(editor) = state.upgrade() {
                    let carried = close_partner(shared.as_ref(), window, cx);
                    show(&editor, owner, (label.clone(), side), carried, window, cx);
                    cx.stop_propagation();
                }
            })
            .capture_action(move |_: &Replace, window, cx| {
                // These readers are immutable. The inherited Replace shortcut
                // must not open the inaccessible private native overlay either.
                if let Some(editor) = replacement.upgrade() {
                    let carried = close_partner(replacement_shared.as_ref(), window, cx);
                    let opening = (replacement_label.clone(), side);
                    show(&editor, owner, opening, carried, window, cx);
                    cx.stop_propagation();
                }
            })
            .capture_action(move |_: &Escape, window, cx| {
                // Its own bar returns focus to this text, where Escape may
                // have come from the bar; the partner's leaves focus here.
                if let Some((own, bar)) = closing
                    .iter()
                    .enumerate()
                    .filter_map(|(at, bar)| Some((at == 0, bar.as_ref()?.upgrade()?)))
                    .find(|(_, bar)| bar.read(cx).open)
                {
                    bar.update(cx, |bar, cx| bar.hide(own, window, cx));
                    cx.stop_propagation();
                }
            })
            // Shift+Tab, which a read-only text takes for outdenting and
            // does nothing with, returns to the open bar searching it.
            .capture_action(move |_: &OutdentInline, window, cx| {
                if let Some(bar) = returning
                    .as_ref()
                    .and_then(WeakEntity::upgrade)
                    .filter(|bar| bar.read(cx).open)
                {
                    let focus = bar.read(cx).focus.clone();
                    FindBar::focus_last(&focus, window, cx);
                    cx.stop_propagation();
                }
            })
            .children(own_bar)
            .child({
                let code = div()
                    // Code scrolls sideways in here: the width walk
                    // (`width_walk`) checks the chrome around it.
                    .debug_selector(|| "code-editor".into())
                    .size_full()
                    .child(native.h(relative(1.)));
                // Painted after the text, which records its scrolled bounds
                // as it paints, so what stands beside it reads that frame's.
                let (width, elements) = beside.unwrap_or_default();
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .pl(width)
                    .child(code)
                    .children(elements)
            })
    }
}

/// The open Find bar of editors that share one ([`Editor::sharing_find_with`]),
/// for their owner to draw across them all, as Split draws it across both
/// sides in place of their headings. Opening one closes the others, so at
/// most one is open.
pub fn shared_bar(editors: &[Entity<EditorState>], cx: &App) -> Option<AnyElement> {
    editors
        .iter()
        .filter_map(|editor| panel(editor, cx))
        .find(|bar| bar.read(cx).open)
        .map(IntoElement::into_any_element)
}

/// The height the open Find bar of `editors` that share one takes drawn
/// `width` across, as layout lays it out, or `None` where none is open: for
/// their owner to choose what gives way to it in the frame that draws it,
/// as Compare's footer does where it would leave fewer than three rows of
/// Split's text in view.
pub fn shared_bar_height(
    editors: &[Entity<EditorState>],
    width: Pixels,
    window: &Window,
    cx: &App,
) -> Option<Pixels> {
    let bar = editors
        .iter()
        .filter_map(|editor| panel(editor, cx))
        .find(|bar| bar.read(cx).open)?;
    Some(bar.read(cx).height_at(width, window, cx))
}

/// What a shared bar carries as it moves to the other side: its field's
/// value and whether it matches case.
struct Carried {
    value: SharedString,
    sensitive: bool,
}

/// Closes the shared bar of `partner`, if open, as Find opens over the
/// editor it shares one with, and hands back its field's value and Match
/// case, which the bar takes there: it is one bar, so it moves with its
/// query. Focus goes to the editor opening its own.
fn close_partner(
    partner: Option<&WeakEntity<EditorState>>,
    window: &mut Window,
    cx: &mut App,
) -> Option<Carried> {
    let bar = partner
        .and_then(WeakEntity::upgrade)
        .and_then(|partner| panel(&partner, cx))
        .filter(|bar| bar.read(cx).open)?;
    let carried = Carried {
        value: bar.read(cx).query.read(cx).value(),
        sensitive: bar.read(cx).reading(cx).0,
    };
    bar.update(cx, |bar, cx| bar.hide(false, window, cx));
    Some(carried)
}

/// Opens `editor`'s Find bar, made on first use with what its field is
/// named after and the side it searches where it is shared, `(label,
/// side)`, taking a moved bar's query and Match case where `carried`.
fn show(
    editor: &Entity<EditorState>,
    owner: EntityId,
    (label, side): (SharedString, Option<Side>),
    carried: Option<Carried>,
    window: &mut Window,
    cx: &mut App,
) {
    let bar = if let Some(bar) = panel(editor, cx) {
        bar
    } else {
        // Weak source ownership allows released previews to disappear. Eviction
        // clears only our decorations; query/case/match cursor live in EditorState.
        let mut panels = std::mem::take(&mut cx.default_global::<Panels>().0);
        panels.retain(|panel| panel.read(cx).editor.upgrade().is_some());
        if panels.len() >= RETAINED_PANELS {
            let candidate = panels
                .iter()
                .position(|panel| !panel.read(cx).open)
                .or_else(|| {
                    panels.iter().position(|panel| {
                        let bar = panel.read(cx);
                        !bar.focus.contains_focused(window, cx)
                            && !bar
                                .editor
                                .upgrade()
                                .is_some_and(|editor| editor.focus_handle(cx).is_focused(window))
                    })
                });
            if let Some(index) = candidate {
                panels
                    .remove(index)
                    .update(cx, |bar, cx| bar.hide(false, window, cx));
            } else {
                cx.default_global::<Panels>().0 = panels;
                return;
            }
        }
        let bar = cx.new(|cx| FindBar::new(editor, owner, label.clone(), side, window, cx));
        panels.push(bar.clone());
        cx.default_global::<Panels>().0 = panels;
        bar
    };
    bar.update(cx, |bar, cx| {
        bar.open = true;
        bar.owner = owner;
        bar.label = label;
        if let Some(Carried { value, sensitive }) = carried {
            bar.carry(value, sensitive, window, cx);
        }
        bar.sync_highlights(cx);
        // The field selects its whole query, as a macOS find field does when
        // Command-F returns to it, and shows it from its start, where its
        // caret stands: the selection runs back from the query's end. The
        // kit follows a caret into view only where the selection leaves
        // some text out, so a query wider than the field, selected from its
        // start, kept the view it last had, and a field narrower than when
        // it closed, or a query taken from the editor's selection, drew its
        // caret past the field's end.
        bar.query.update(cx, |query, cx| {
            query.focus(window, cx);
            query.set_selected_range(query.value().len()..0, cx);
            query.set_scroll_offset(gpui_kit::point(px(0.), px(0.)), cx);
        });
        cx.notify();
        App::notify(cx, owner);
    });
}

struct FindBar {
    editor: WeakEntity<EditorState>,
    owner: EntityId,
    label: SharedString,
    /// The side it searches, where it is a bar two editors share
    /// ([`Editor::sharing_find_with`]).
    side: Option<Side>,
    query: Entity<InputState>,
    focus: FocusHandle,
    /// Match case's focus, which its checkbox and its "Aa" toggle share, so
    /// a resize that changes its form keeps focus on it.
    case_focus: FocusHandle,
    open: bool,
    height: Pixels,
    decorations: TextDecorationCollection,
    background_available: bool,
    painted: Option<PaintedMatches>,
    _subscriptions: Vec<Subscription>,
}
struct PaintedMatches {
    ranges: Rc<Vec<Range<usize>>>,
    current: usize,
    colors: MatchColors,
}
/// Find's match backgrounds, fitted with the palette ([`crate::appearance::EditorHighlights`]),
/// and the current match's underline.
#[derive(Clone, Copy, PartialEq, Eq)]
struct MatchColors {
    other: u32,
    current: u32,
    underline: u32,
}
impl FindBar {
    fn new(
        editor: &Entity<EditorState>,
        owner: EntityId,
        label: SharedString,
        side: Option<Side>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let initial = {
            let source = editor.read(cx);
            let session = source.search_session();
            if session.query.is_empty() {
                let selection = source.selected_text();
                if source.selected_range().len() <= MAX_QUERY_BYTES {
                    selection.to_string()
                } else {
                    String::new()
                }
            } else {
                session.query.clone()
            }
        };
        let reserved = reserved_layer(editor, cx);
        let background_available = reserved.is_some();
        let decorations = editor.update(cx, |source, cx| {
            // The native overlay is private and lacks labels. Keep its engine
            // closed; its query and match navigation remain public and usable.
            source.close_search(cx);
            if source.search_session().query != initial {
                source.set_search_query(
                    initial.clone(),
                    source.search_session().case_insensitive,
                    cx,
                );
            }
            reserved.unwrap_or_else(|| source.create_decorations_collection(Vec::new(), cx))
        });
        // A shared bar fits its placeholder to its field ([`Controls`]).
        let placeholder = side.map_or("Find in file", |side| side.placeholders()[0]);
        let query = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder)
                .default_value(initial)
        });
        let tabbing = cx.weak_entity();
        let subscriptions = vec![
            cx.subscribe(&query, |this, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    this.update_query(cx);
                }
            }),
            cx.observe(editor, |this, _, cx| {
                if this.open {
                    this.sync_highlights(cx);
                }
            }),
            // Tab out of the bar goes to the text it searches, After's while
            // it searches After, though Before's text follows the bar in the
            // window's order of controls, the bar standing above both sides.
            // The toolkit's Tab action is private to it, so the key is taken
            // before the window resolves it.
            cx.intercept_keystrokes(move |event, window, cx| {
                let keystroke = &event.keystroke;
                if keystroke.key != "tab" || keystroke.modifiers.modified() {
                    return;
                }
                let Some((focus, editor)) = tabbing.upgrade().and_then(|bar| {
                    let bar = bar.read(cx);
                    bar.open.then(|| (bar.focus.clone(), bar.editor.clone()))
                }) else {
                    return;
                };
                if !focus.contains_focused(window, cx) {
                    return;
                }
                window.focus_next(cx);
                if !focus.contains_focused(window, cx)
                    && let Some(editor) = editor.upgrade()
                {
                    editor.focus_handle(cx).focus(window, cx);
                }
                cx.stop_propagation();
            }),
        ];
        Self {
            editor: editor.downgrade(),
            owner,
            label,
            side,
            query,
            focus: cx.focus_handle(),
            case_focus: cx.focus_handle(),
            open: false,
            height: px(0.),
            decorations,
            background_available,
            painted: None,
            _subscriptions: subscriptions,
        }
    }
    fn update_query(&mut self, cx: &mut Context<Self>) {
        let value = self.query.read(cx).value();
        let query = bounded_query(value.as_str());
        if let Some(editor) = self.editor.upgrade() {
            editor.update(cx, |source, cx| {
                let insensitive = source.search_session().case_insensitive;
                source.set_search_query(query, insensitive, cx);
            });
        }
        self.sync_highlights(cx);
        cx.notify();
    }
    fn navigate(&mut self, previous: bool, cx: &mut Context<Self>) {
        if let Some(editor) = self.editor.upgrade() {
            editor.update(cx, |source, cx| {
                if previous {
                    source.previous_search_match(cx);
                } else {
                    source.next_search_match(cx);
                }
            });
        }
        self.sync_highlights(cx);
        cx.notify();
    }
    /// Matches case, or not, as `sensitive` says.
    fn match_case(&mut self, sensitive: bool, cx: &mut Context<Self>) {
        if let Some(editor) = self.editor.upgrade() {
            editor.update(cx, |source, cx| {
                source.set_search_query(source.search_session().query.clone(), !sensitive, cx)
            });
        }
        self.sync_highlights(cx);
        cx.notify();
    }
    /// Takes `value` into the field and the search, matching case where
    /// `sensitive`, as the bar moves here from the other side.
    fn carry(
        &mut self,
        value: SharedString,
        sensitive: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.query.update(cx, |query, cx| {
            if query.value() != value {
                query.set_value(value.clone(), window, cx);
            }
        });
        if let Some(editor) = self.editor.upgrade() {
            editor.update(cx, |source, cx| {
                source.set_search_query(bounded_query(value.as_str()), !sensitive, cx)
            });
        }
    }
    /// Focuses the last control of the bar whose box tracks `bar`, as
    /// Shift+Tab from the text it searches does: the last of its controls in
    /// the window's order, which a wrapped bar draws on its last line.
    fn focus_last(bar: &FocusHandle, window: &mut Window, cx: &mut App) {
        // Its box holds no stop of its own and comes before its controls,
        // which it walks to the last.
        let from = window.focused(cx);
        window.focus(bar, cx);
        let mut last = None;
        for _ in 0..=ONE_LINE.len() {
            window.focus_next(cx);
            if !bar.contains_focused(window, cx) {
                break;
            }
            last = window.focused(cx);
        }
        if let Some(to) = last.or(from) {
            window.focus(&to, cx);
        }
    }
    fn hide(&mut self, focus_source: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        self.height = px(0.);
        self.painted = None;
        self.decorations.clear(cx);
        set_match_ranges(&self.editor, Vec::new(), !self.background_available, cx);
        if focus_source && let Some(editor) = self.editor.upgrade() {
            editor.focus_handle(cx).focus(window, cx);
        }
        App::notify(cx, self.owner);
        cx.notify();
    }
    fn sync_highlights(&mut self, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let session = editor.read(cx).search_session();
        let ranges = session.matcher.matched_ranges();
        let current = session.matcher.current_match_index();
        let highlights = editor_highlights(cx);
        let colors = MatchColors {
            other: highlights.find_match,
            current: highlights.find_current,
            underline: palette(cx).accent,
        };
        if self.painted.as_ref().is_some_and(|painted| {
            Rc::ptr_eq(&painted.ranges, &ranges)
                && painted.current == current
                && painted.colors == colors
        }) {
            return;
        }
        let decorations: Vec<_> = highlight_window(ranges.len(), current)
            .map(|index| {
                let active = index == current;
                TextDecoration::new(
                    ranges[index].clone(),
                    match_style(active, self.background_available, colors),
                )
            })
            .collect();
        set_match_ranges(
            &self.editor,
            decorations
                .iter()
                .map(|decoration| decoration.range.clone())
                .collect(),
            !self.background_available,
            cx,
        );
        self.painted = Some(PaintedMatches {
            ranges,
            current,
            colors,
        });
        self.decorations.set(decorations, cx);
        cx.notify();
    }
}

impl FindBar {
    /// Whether Find matches case, the count it reads ("3/10") and whether
    /// it found nothing.
    fn reading(&self, cx: &App) -> (bool, SharedString, bool) {
        self.editor
            .upgrade()
            .map(|editor| {
                let session = editor.read(cx).search_session();
                (
                    !session.case_insensitive,
                    session.matcher.label().into(),
                    session.matcher.is_empty(),
                )
            })
            .unwrap_or((false, "0/0".into(), true))
    }

    /// Whether the query is past Find's limit, which the bar states below
    /// its controls.
    fn too_long(&self, cx: &App) -> bool {
        self.query.read(cx).value().len() > MAX_QUERY_BYTES
    }

    /// The bar's box. Spanning the line numbers, it stands its controls at
    /// a pane's content edge (`appearance::PANE_INSET`), where Split's side
    /// headings start their text.
    fn frame() -> gpui_kit::Div {
        div()
            .w_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap_1()
            .px(crate::appearance::ui_size(crate::appearance::PANE_INSET))
            .py_1()
            .border_b_1()
            .text_size(crate::appearance::ui_text(11.))
    }

    /// The measure of the bar's controls reading `count`: the gap between
    /// them, and the compact control's 28 px line they take whatever their
    /// forms, so a form never moves the code.
    ///
    /// The gap and the line are each on their whole device pixel, halves
    /// toward zero, as layout draws each gap and each line
    /// (`Window::pixel_snap`), so the plan's lines sum the gaps drawn
    /// between their parts and the lines drawn: summed from the raw
    /// 3.25 px at 13 pt, the plan's five gaps on one line were 0.42 px
    /// short of the 4 device pixels layout draws each at scale 1.2 and
    /// 0.25 px over them at 1.25, so the line a form was chosen by was not
    /// the line drawn; and the raw line, 38.77 px at 18 pt, drawn on 48
    /// device pixels at 1.25, made the bar's box 49 ([`Plan::height`]).
    fn fit(count: SharedString, window: &Window) -> Fit {
        Fit {
            count,
            gap: window.pixel_snap(window.rem_size() * 0.25),
            row: window.pixel_snap(crate::appearance::ui_size(28.)),
        }
    }

    /// The limit's notice below the controls.
    fn limit_notice() -> gpui_kit::Div {
        div().child(LIMIT_NOTICE)
    }

    /// The height the bar takes `width` across, as layout draws it on this
    /// display's device pixels ([`Self::frame`], [`Fitted`]): its controls'
    /// lines inside its pane insets ([`Plan::height`]), rounded up to a
    /// device pixel as layout sizes a measured element; its padding, and
    /// the gap before the limit's notice where it states it, each a quarter
    /// rem on its whole device pixel, halves toward zero; its 1 px rule on
    /// at least one device pixel; and the notice as layout lays a text out
    /// ([`Self::notice_height`]), rounded up to a device pixel. Summed from
    /// the raw quarter rem and 1 px, the bar reckoned at 66.75 px for two
    /// lines at 13 pt was drawn 67.5 px at scale 1.2 and 66.5 at scale 2;
    /// and the notice, reckoned as one line of a whole pixel rounded up to a
    /// device pixel, was a device pixel over the line drawn at 13 pt at 1.2
    /// and 1.25, and two lines short in a 461 px window at 13 pt, where it
    /// wraps on three.
    fn height_at(&self, width: Pixels, window: &Window, cx: &App) -> Pixels {
        let fit = Self::fit(self.reading(cx).1, window);
        let widths = Widths::measure(&fit.count, fit.gap, window);
        let scale = window.scale_factor();
        // GPUI's `ceil_to_device_pixel`, as it sizes a measured element.
        let measured = |length: Pixels| px((f32::from(length) * scale).ceil() / scale);
        let inset = window.pixel_snap(crate::appearance::ui_size(crate::appearance::PANE_INSET));
        let room = (width - inset * 2.).max(px(0.));
        let controls = planned_width(room, scale);
        // `py_1` above and below, and `gap_1` before the notice.
        let padding = window.pixel_snap(window.rem_size() * 0.25);
        let rule = window.pixel_snap(px(1.)).max(px(1. / scale));
        let lines = measured(widths.plan(controls).height(fit.row, fit.gap, window));
        let notice = if self.too_long(cx) {
            padding + measured(Self::notice_height(room, window))
        } else {
            px(0.)
        };
        lines + padding * 2. + rule + notice
    }

    /// The height the limit's notice takes `room` wide, as GPUI lays a text
    /// out (`TextLayout::layout`): its lines wrapped at that width, each
    /// line on its whole device pixel, halves toward zero, before layout
    /// rounds the text's height up to a device pixel.
    fn notice_height(room: Pixels, window: &Window) -> Pixels {
        // The style where `views::split_room`'s frame runs, outside the bar,
        // which holds inside it: the bar sets only the text's size (and colour).
        let mut style = window.text_style();
        style.font_size = crate::appearance::ui_text(11.).into();
        let rem = window.rem_size();
        let size = style.font_size.to_pixels(rem);
        let line = window.pixel_snap(style.line_height.to_pixels(size.into(), rem));
        let text = SharedString::new_static(LIMIT_NOTICE);
        let run = style.to_run(text.len());
        match window
            .text_system()
            .shape_text(text, size, &[run], Some(room), None)
        {
            Ok(lines) => lines
                .iter()
                .fold(px(0.), |height, wrapped| height + wrapped.size(line).height),
            Err(_) => line,
        }
    }
}

impl Render for FindBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_highlights(cx);
        let (case_sensitive, count, empty) = self.reading(cx);
        let colors = palette(cx);
        let panel = cx.weak_entity();
        let enter = cx.weak_entity();
        let escape = cx.weak_entity();
        let dismiss = cx.weak_entity();
        let again = cx.weak_entity();
        let bar = Self::frame()
            .id("accessible-editor-find")
            .debug_selector(|| "file-find-bar".into())
            .track_focus(&self.focus)
            .bg(rgb(colors.panel))
            .border_color(rgb(colors.border))
            .text_color(rgb(colors.text))
            .capture_action(move |action: &Enter, _, cx| {
                if action.secondary {
                    return;
                }
                let _ = enter.update(cx, |bar, cx| bar.navigate(action.shift, cx));
                cx.stop_propagation();
            })
            .capture_action(move |_: &Escape, window, cx| {
                let _ = escape.update(cx, |bar, cx| bar.hide(true, window, cx));
                cx.stop_propagation();
            })
            // Escape on any of the bar's controls closes Find as Escape in
            // its field does, focus returning to the text it searches. Only
            // the field binds the key to the kit's Escape; off it the key is
            // the app's Escape (`ClearSearch`), Back from Compare, so Escape
            // on Next left Compare for History.
            .on_action(move |_: &crate::ClearSearch, window, cx| {
                let _ = dismiss.update(cx, |bar, cx| bar.hide(true, window, cx));
                cx.stop_propagation();
            })
            // Primary+F in the bar returns to its field, selecting its query,
            // as it does from the text; a bar its editors' owner draws across
            // them (`shared_bar`) stands outside the text's element, which
            // catches it there.
            .capture_action(move |_: &Search, window, cx| {
                let reopening = again.upgrade().and_then(|bar| {
                    let bar = bar.read(cx);
                    Some((
                        bar.editor.upgrade()?,
                        bar.owner,
                        (bar.label.clone(), bar.side),
                    ))
                });
                if let Some((editor, owner, opening)) = reopening {
                    show(&editor, owner, opening, None, window, cx);
                    cx.stop_propagation();
                }
            })
            // The controls take their forms from the bar's width in the frame
            // it is drawn ([`Controls`]).
            .child(Fitted {
                fit: Self::fit(count.clone(), window),
                controls: Controls {
                    bar: cx.weak_entity(),
                    query: self.query.clone(),
                    label: self.label.clone(),
                    side: self.side,
                    case_focus: self.case_focus.clone(),
                    case_sensitive,
                    count,
                    empty,
                    muted: colors.muted,
                },
            })
            .children(
                self.too_long(cx)
                    .then(|| Self::limit_notice().text_color(rgb(colors.warning))),
            )
            .on_prepaint(move |bounds, _, cx| {
                let _ = panel.update(cx, |bar, cx| {
                    if bar.open && bar.height != bounds.size.height {
                        bar.height = bounds.size.height;
                        App::notify(cx, bar.owner);
                    }
                });
            });
        // A shared bar marks the side it searches: that side's half of its
        // 1 px rule in the accent colour, which says which side it searches
        // whatever its field holds. A box paints its border after its
        // children, so the mark stands after the bar, over the bar's box,
        // and is drawn as the rule is, a bottom border of that box, on the
        // same device pixel rows at every scale: a child of the bar, it was
        // painted under the bar's own rule and never showed.
        div()
            .w_full()
            .flex_none()
            .relative()
            .flex()
            .flex_col()
            .child(bar)
            .children(self.side.map(|side| {
                div()
                    .debug_selector(|| "file-find-side".into())
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .w(relative(0.5))
                    .left(relative(if side == Side::Before { 0. } else { 0.5 }))
                    .border_b_1()
                    .border_color(rgb(colors.accent))
            }))
    }
}

/// What the bar states below its controls while its query is past Find's
/// limit ([`MAX_QUERY_BYTES`]).
const LIMIT_NOTICE: &str = "Find is limited to 4 KiB; shorten this query.";

/// The text whose width is the Find field's floor: eight digits, about eight
/// characters of a query at the field's text size.
const FIELD_FLOOR: &str = "00000000";

/// The size the field draws its text at, at the default interface size: the
/// kit's small field's 0.875 rem.
const FIELD_TEXT: f32 = 13. * 0.875;

/// The count's least reserved width, so the forms do not change as a query
/// typed into the field finds its first hundred matches.
const COUNT_ROOM: &str = "000/000";

/// The find bar's controls, laid out for the bar's width in the frame it is
/// drawn ([`crate::in_frame`]): Close, Previous and Next each draw their
/// label, or their icon in the compact frame's 28 px square, scaled, with the
/// same accessible name and their label as their tooltip
/// ([`crate::system_preview::HeaderControl`]), and Match case its checkbox
/// or an "Aa" toggle named Match case, by [`Widths::forms`]. Each keeps its
/// focus through a change of form.
struct Controls {
    bar: WeakEntity<FindBar>,
    query: Entity<InputState>,
    label: SharedString,
    /// The side a shared bar searches, which its empty field names.
    side: Option<Side>,
    case_focus: FocusHandle,
    case_sensitive: bool,
    count: SharedString,
    empty: bool,
    muted: u32,
}

/// What the bar's controls are measured by: the count they read, the gap
/// between them and the height of their lines.
#[derive(Clone)]
struct Fit {
    count: SharedString,
    gap: Pixels,
    row: Pixels,
}

/// What the find bar's parts take on a row, as layout draws them on this
/// display's device pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Widths {
    /// The field's floor: [`FIELD_FLOOR`] at its text size, inside its
    /// padding and border.
    floor: Pixels,
    close: Pixels,
    previous: Pixels,
    next: Pixels,
    /// Match case's checkbox: its box, the gap after it and its label.
    case: Pixels,
    /// Its "Aa" toggle.
    toggle: Pixels,
    /// The count, at least [`COUNT_ROOM`].
    count: Pixels,
    /// An icon form's square.
    icon: Pixels,
    gap: Pixels,
}

/// Which of the find bar's controls draw their labels: Close; Previous and
/// Next, which take one form together; and Match case, its checkbox, or
/// else its "Aa" toggle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Forms {
    pub(crate) close: bool,
    pub(crate) pair: bool,
    pub(crate) case: bool,
}

/// The find bar's texts at the sizes it lays them out at, in whole pixels
/// ([`crate::views::line_widths`]), which [`Widths::measure`] places on the
/// display's device pixels on every frame: kept by the font and sizes they
/// were laid out at, which do not change with the display's scale.
#[derive(Clone, Copy)]
struct Labels {
    /// [`FIELD_FLOOR`] at the field's 0.875 rem.
    floor: Pixels,
    /// Close, Previous, Next and the "Aa" toggle's labels at a compact
    /// Button's 12 pt ([`crate::widest_label_now`]).
    close: Pixels,
    previous: Pixels,
    next: Pixels,
    toggle: Pixels,
    /// "Match case" at its checkbox's 1 rem.
    case: Pixels,
    /// [`COUNT_ROOM`] at the count's 11 pt.
    count: Pixels,
}

thread_local! {
    /// [`Labels`], by the font and sizes they were laid out at.
    static LABELS: RefCell<Option<(crate::views::LineKey, Labels)>> = const { RefCell::new(None) };
    /// The count's text in whole pixels, by the same.
    static COUNT: crate::views::TextWidth = const { RefCell::new(None) };
}

#[cfg(test)]
thread_local! {
    /// The forms, the field's least width and the lines the find bar was
    /// last drawn with, for a test.
    pub(crate) static DRAWN_FORMS: RefCell<Option<(Forms, Pixels, usize)>> = const { RefCell::new(None) };
    /// The widths the find bar's plan was last drawn from, for a test.
    static DRAWN_WIDTHS: RefCell<Option<Widths>> = const { RefCell::new(None) };
}

impl Widths {
    /// The parts' widths in the font the bar lays them out in, with `count`
    /// as the count and `gap` between parts, as layout places them on this
    /// display's device pixels. Their texts are laid out once for each font
    /// and size ([`Labels`]) and placed here on every frame, by arithmetic,
    /// so a window moved to a display of another scale takes that scale's
    /// widths; the font and sizes are read once
    /// ([`crate::views::LineKey::current`]): each read builds GPUI's
    /// default font features, and the bar is measured every frame.
    ///
    /// Close, Previous and Next take the width layout places their labels
    /// at (`crate::placed_button_width`), which a labelled one is drawn at
    /// ([`crate::system_preview::HeaderControl::button`]), and the bar's
    /// forms and lines are chosen from those widths. Sized as the unrounded
    /// sum of their insets and labels rounded up to a device pixel, each was
    /// drawn a device pixel narrower than that at some size from 11 to
    /// 18 pt at scales 1.1, 1.2, 1.25, 1.5 and 1.75 (Close 78.33 px where
    /// layout places its label at 79.17, at 1.2 and 18 pt), and the widths
    /// kept were the last display's, the key holding no display scale.
    fn measure(count: &str, gap: Pixels, window: &Window) -> Self {
        let small = crate::appearance::ui_text(11.);
        let key = crate::views::LineKey::current(window);
        let count = COUNT.with_borrow_mut(|cached| match cached {
            Some((laid_out, text, width)) if *laid_out == key && text == count => *width,
            _ => {
                let width = crate::views::line_widths(&[(count, small)], window);
                *cached = Some((key.clone(), count.to_owned(), width));
                width
            }
        });
        let labels = crate::views::cached_width(&LABELS, &key, || {
            let rem = window.rem_size();
            let button = |label: &str| crate::widest_label_now(&[label], window);
            Labels {
                floor: crate::views::line_widths(&[(FIELD_FLOOR, rem * 0.875)], window),
                close: button("Close"),
                previous: button("Previous"),
                next: button("Next"),
                toggle: button("Aa"),
                case: crate::views::line_widths(&[("Match case", rem)], window),
                count: crate::views::line_widths(&[(COUNT_ROOM, small)], window),
            }
        });
        let scale = window.scale_factor();
        let device = |width: Pixels| px((f32::from(width) * scale).ceil() / scale);
        let text = |width: Pixels| crate::views::laid_out_text_width(width, window);
        let button = |label: Pixels| crate::placed_button_width(label, false, window);
        let rem = window.rem_size();
        Self {
            // The kit's small field: 8 px of padding and a 1 px border
            // either side of its text, at 0.875 rem.
            floor: device(text(labels.floor) + px(18.)),
            close: button(labels.close),
            previous: button(labels.previous),
            next: button(labels.next),
            // The kit's checkbox: a 1 rem box and 0.5 rem before its label
            // at 1 rem, the box and the gap each on its whole device pixel
            // as layout draws them; rounded up together, they were a
            // device pixel wider than drawn at scale 1.25 (25 for 24 at
            // 13 pt).
            case: window.pixel_snap(rem) + window.pixel_snap(rem * 0.5) + text(labels.case),
            toggle: button(labels.toggle),
            count: text(labels.count.max(count)),
            // The icon form's 28 px square, on its device pixel as GPUI
            // rounds a length before layout.
            icon: crate::image_compare::on_device_pixel(
                window.pixel_snap(crate::appearance::ui_size(28.)),
                window,
            ),
            gap,
        }
    }

    /// What `part` takes across in `forms`, in a bar whose controls have
    /// `room`: the field its floor, or the whole room where that is less.
    fn width(&self, part: Part, forms: Forms, room: Pixels) -> Pixels {
        match part {
            Part::Field => self.floor.min(room),
            Part::Close if forms.close => self.close,
            Part::Case if forms.case => self.case,
            Part::Case => self.toggle,
            Part::Count => self.count,
            Part::Previous if forms.pair => self.previous,
            Part::Next if forms.pair => self.next,
            Part::Close | Part::Previous | Part::Next => self.icon,
        }
    }

    /// What the parts take on one line in `forms`: the field's floor, every
    /// control and the gaps between them.
    fn line(&self, forms: Forms) -> Pixels {
        ONE_LINE
            .iter()
            .map(|part| self.width(*part, forms, Pixels::MAX))
            .fold(self.gap * (ONE_LINE.len() - 1) as f32, |sum, width| {
                sum + width
            })
    }

    /// The forms a bar whose controls have `room` across takes, and whether
    /// they lie on one line, in the one responsive order: the bar keeps one
    /// line, which moves no code, while the field's floor and every control
    /// fit it, its controls giving up their labels for it, Previous and Next
    /// together first, then Match case its checkbox for its "Aa" toggle,
    /// then Close; past that, every control keeps its icon form and the bar
    /// wraps them whole. The field keeps its floor.
    fn forms(&self, room: Pixels) -> (Forms, bool) {
        let icons = |pair, case, close| Forms { close, pair, case };
        [
            icons(true, true, true),
            icons(false, true, true),
            icons(false, false, true),
            icons(false, false, false),
        ]
        .into_iter()
        .find(|forms| self.line(*forms) <= room + FIT_SLACK)
        .map_or((icons(false, false, false), false), |forms| (forms, true))
    }

    /// Where each part lies in a bar whose controls have `room` across: on
    /// one line, in [`ONE_LINE`]'s order, where they fit it ([`Self::forms`]);
    /// otherwise the field on the first line, with Close where it fits
    /// beside the field's floor, Match case starting the line after the
    /// field's, and every part after it, in its order, on the line it fits
    /// whole on, Previous and Next always together, so a line that cannot
    /// hold the next part wraps it below. A bar narrower than the field's
    /// floor gives the field the line's whole room, the most it has.
    fn plan(&self, room: Pixels) -> Plan {
        let (forms, one_line) = self.forms(room);
        let width = |part: Part| self.width(part, forms, room);
        let lines = if one_line {
            vec![ONE_LINE.to_vec()]
        } else {
            let mut lines = vec![vec![Part::Field]];
            let mut used = width(Part::Field);
            for unit in [
                &[Part::Close][..],
                &[Part::Case],
                &[Part::Count],
                &[Part::Previous, Part::Next],
            ] {
                let across = unit
                    .iter()
                    .map(|part| width(*part))
                    .fold(self.gap * (unit.len() - 1) as f32, |sum, width| sum + width);
                let beside_field = lines.len() == 1;
                if (unit[0] == Part::Case && beside_field)
                    || used + self.gap + across > room + FIT_SLACK
                {
                    lines.push(unit.to_vec());
                    used = across;
                } else {
                    lines.last_mut().expect("a line").extend_from_slice(unit);
                    used += self.gap + across;
                }
            }
            lines
        };
        Plan {
            forms,
            lines,
            field: width(Part::Field),
        }
    }
}

/// How far the parts' summed widths may run past the room they are
/// compared with and still fit it: the float error of a sum of lengths
/// placed on device pixels, far under one. Compared without it, Match
/// case's checkbox line at 13 pt and scale 1.25 summed 345.60004 px, over
/// the 345.59998 px of the 432 device pixels it fills, and the bar gave the
/// checkbox up a device pixel early.
const FIT_SLACK: Pixels = px(1e-3);

/// The debug selectors of the bar's lines, in their order, which a test
/// reads each line's bounds by: a bar takes at most one line a part.
const LINE_SELECTORS: [&str; 5] = [
    "file-find-line-1",
    "file-find-line-2",
    "file-find-line-3",
    "file-find-line-4",
    "file-find-line-5",
];

/// The parts in their order on a bar of one line: the field, then Match
/// case, the count with Previous and Next, and Close at the line's end, as
/// it stands at the first line's end on a wrapped bar.
const ONE_LINE: [Part; 6] = [
    Part::Field,
    Part::Case,
    Part::Count,
    Part::Previous,
    Part::Next,
    Part::Close,
];

/// A part of the find bar, in its order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Field,
    Close,
    Case,
    Count,
    Previous,
    Next,
}

/// The find bar's forms and lines for one width ([`Widths::plan`]).
#[derive(Clone, Debug, PartialEq)]
struct Plan {
    forms: Forms,
    lines: Vec<Vec<Part>>,
    /// The field's least width.
    field: Pixels,
}

impl Plan {
    /// The height its lines take, `row` each, `gap` apart, each on whole
    /// device pixels as layout draws them ([`FindBar::fit`]), as the size of
    /// a measured element that layout lays out on those device pixels: it
    /// rounds a measured size up to a device pixel (GPUI's
    /// `ceil_to_device_pixel`), and whole device pixels divided by the scale
    /// can multiply back a float's error over them, which it would round up
    /// to one more (99 device pixels at 1.2, two lines at 18 pt, came back
    /// 99.00001). Summed from the raw 28 px row, the bar's box was a device
    /// pixel taller than the lines drawn in it, below the last, at 18 pt at
    /// scales 1.25 and 1.5.
    fn height(&self, row: Pixels, gap: Pixels, window: &Window) -> Pixels {
        let lines = self.lines.len() as f32;
        let height =
            crate::image_compare::on_device_pixel(row * lines + gap * (lines - 1.), window);
        let scale = window.scale_factor();
        let device = (f32::from(height) * scale).round();
        let mut measured = f32::from(height);
        while measured * scale > device {
            measured = measured.next_down();
        }
        px(measured)
    }
}

/// The find bar's controls as an element: as tall as the lines they take at
/// the width layout gives them ([`Widths::plan`]), and built for that width
/// as they are prepainted, so a resize lays the bar out in the frame it is
/// drawn. Built a frame later, from the width the frame before measured, a
/// narrowing window would first draw the wider frame's forms cut.
///
/// Its height and its lines come from one width ([`Planned`]): the width
/// layout measured it at, on the whole device pixel at or below it. Layout
/// measures it at its unrounded width and then draws it on device pixels,
/// rounding each edge, so its drawn width lies up to a device pixel either
/// side of the measured one; planned from the drawn width, the lines missed
/// the height. On Split's first side at 13 pt in a 572.5 × 490 window at
/// scale 2 the measure planned three lines from 104.25 px and the controls
/// took four from the 104 px drawn, their last line below the bar, over the
/// code. The drawn width is never less than the planned one, so the planned
/// lines fit it.
///
/// It is no [`crate::in_frame::InFrame`], which builds what it holds in the
/// frame it is drawn too: an `InFrame` takes no height of its own, and the
/// bar takes the height of its lines.
struct Fitted {
    fit: Fit,
    controls: Controls,
}

/// The width a [`Fitted`]'s measure planned its lines from, which its
/// prepaint plans them from too.
type Planned = Rc<std::cell::Cell<Option<Pixels>>>;

/// The width a find bar's lines are planned from where layout measures it
/// `width` wide on a display of `scale`: the whole device pixel at or below
/// it, which the width drawn holds whichever device pixel each of its edges
/// rounds to.
fn planned_width(width: Pixels, scale: f32) -> Pixels {
    px((f32::from(width) * scale + 1e-3).floor() / scale)
}

impl IntoElement for Fitted {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl gpui_kit::Element for Fitted {
    type RequestLayoutState = Option<(Widths, Planned)>;
    type PrepaintState = Option<AnyElement>;

    fn id(&self) -> Option<gpui_kit::ElementId> {
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
        _: &mut App,
    ) -> (gpui_kit::LayoutId, Option<(Widths, Planned)>) {
        // Measured here, in the text style the bar gives its controls; the
        // layout's own measure runs outside it.
        let widths = Widths::measure(&self.fit.count, self.fit.gap, window);
        let (row, scale) = (self.fit.row, window.scale_factor());
        let planned = Planned::default();
        let style = gpui_kit::Style {
            size: gpui_kit::Size {
                width: relative(1.).into(),
                height: gpui_kit::Length::Auto,
            },
            ..Default::default()
        };
        let measured = planned.clone();
        let id = window.request_measured_layout(style, move |known, available, window, _| {
            let width = known.width.unwrap_or(match available.width {
                gpui_kit::AvailableSpace::Definite(width) => width,
                _ => px(0.),
            });
            let lines = planned_width(width, scale);
            // The width layout settles on is a known one; the others ask
            // what the bar would take at widths it is not given.
            if known.width.is_some() {
                measured.set(Some(lines));
            }
            gpui_kit::size(width, widths.plan(lines).height(row, widths.gap, window))
        });
        (id, Some((widths, planned)))
    }

    fn prepaint(
        &mut self,
        _: Option<&gpui_kit::GlobalElementId>,
        _: Option<&gpui_kit::InspectorElementId>,
        bounds: gpui_kit::Bounds<Pixels>,
        measured: &mut Option<(Widths, Planned)>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let (widths, planned) = measured.as_ref()?;
        // The width the measure planned from, which the drawn width holds,
        // exceeding it by less than a device pixel and a float's error; a
        // layout that measured no width it was given plans from the drawn.
        let drawn = bounds.size.width;
        let scale = window.scale_factor();
        let slack = FIT_SLACK;
        let width = planned
            .get()
            .filter(|planned| {
                *planned <= drawn + slack && drawn - *planned < px(1. / scale) + slack
            })
            .unwrap_or_else(|| planned_width(drawn, scale));
        let mut element = self.controls.rows(widths.plan(width), *widths, &self.fit);
        element.layout_as_root(
            gpui_kit::size(
                gpui_kit::AvailableSpace::Definite(bounds.size.width),
                gpui_kit::AvailableSpace::Definite(bounds.size.height),
            ),
            window,
            cx,
        );
        element.prepaint_at(bounds.origin, window, cx);
        Some(element)
    }

    fn paint(
        &mut self,
        _: Option<&gpui_kit::GlobalElementId>,
        _: Option<&gpui_kit::InspectorElementId>,
        _: gpui_kit::Bounds<Pixels>,
        _: &mut Option<(Widths, Planned)>,
        element: &mut Option<AnyElement>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(element) = element {
            element.paint(window, cx);
        }
    }
}

impl Controls {
    /// The bar's lines as `plan` places its parts, `widths` wide.
    fn rows(&self, plan: Plan, widths: Widths, fit: &Fit) -> AnyElement {
        use crate::system_preview::HeaderControl;
        let forms = plan.forms;
        #[cfg(test)]
        {
            DRAWN_FORMS
                .with_borrow_mut(|drawn| *drawn = Some((forms, plan.field, plan.lines.len())));
            DRAWN_WIDTHS.with_borrow_mut(|drawn| *drawn = Some(widths));
        }
        // Each control is handed the room that gives it the form chosen
        // for it ([`HeaderControl::button`] labels a control where its
        // labelled form fits the room).
        let room = |labelled: bool, widest: Pixels| if labelled { widest } else { px(0.) };
        let (on_close, on_previous, on_next) =
            (self.bar.clone(), self.bar.clone(), self.bar.clone());
        let close = HeaderControl::new(
            "close-file-find",
            "close-file-find".into(),
            "Close",
            "Close Find and return to file text",
            "close",
            widths.close,
        )
        .button(room(forms.close, widths.close))
        .flex_shrink_0()
        .on_click(move |_, window, cx| {
            let _ = on_close.update(cx, |bar, cx| bar.hide(true, window, cx));
        });
        let previous = HeaderControl::new(
            "previous-file-match",
            "previous-file-match".into(),
            "Previous",
            format!(
                "Previous Find match, {}",
                crate::shortcuts::shifted(crate::shortcuts::ENTER)
            ),
            "chevron",
            widths.previous,
        )
        .button(room(forms.pair, widths.previous));
        // The chevron turned to point back, as Next's points on.
        let previous = if forms.pair {
            previous
        } else {
            previous.icon(
                gpui_kit::component::Icon::default()
                    .path("icons/chevron.svg")
                    .size(crate::appearance::ui_size(16.))
                    .rotate(gpui_kit::percentage(0.5)),
            )
        }
        .flex_shrink_0()
        .disabled(self.empty)
        .on_click(move |_, _, cx| {
            let _ = on_previous.update(cx, |bar, cx| bar.navigate(true, cx));
        });
        let next = HeaderControl::new(
            "next-file-match",
            "next-file-match".into(),
            "Next",
            format!("Next Find match, {}", crate::shortcuts::ENTER),
            "chevron",
            widths.next,
        )
        .button(room(forms.pair, widths.next))
        .flex_shrink_0()
        .disabled(self.empty)
        .on_click(move |_, _, cx| {
            let _ = on_next.update(cx, |bar, cx| bar.navigate(false, cx));
        });
        let (bar, sensitive) = (self.bar.clone(), self.case_sensitive);
        let toggle = move |cx: &mut App| {
            let _ = bar.update(cx, |bar, cx| bar.match_case(!sensitive, cx));
        };
        let case = if forms.case {
            match_case_box(&self.case_focus, sensitive)
                .on_click(move |_: &bool, _, cx| toggle(cx))
                .into_any_element()
        } else {
            match_case_toggle(&self.case_focus, sensitive)
                .on_click(move |_, _, cx| toggle(cx))
                .into_any_element()
        };
        // A shared bar's empty field names the side it searches, its name
        // alone where "Find in" does not fit beside it, as at the field's
        // floor (`workspace::placeholder_fit`).
        let field = div()
            .debug_selector(|| "file-find-field".into())
            .relative()
            .flex_1()
            .min_w(plan.field)
            .child(
                Input::new(&self.query)
                    .small()
                    .aria_label(format!("Find in {}", self.label)),
            )
            .children(self.side.map(|side| {
                crate::workspace::placeholder_fit(
                    self.query.clone(),
                    side.fit(),
                    side.placeholders(),
                    FIELD_TEXT,
                )
            }));
        let count = div()
            .debug_selector(|| "file-find-count".into())
            .flex_shrink_0()
            .text_color(rgb(self.muted))
            .child(self.count.clone());
        let mut parts = [
            Some(field.into_any_element()),
            Some(close.into_any_element()),
            Some(case),
            Some(count.into_any_element()),
            Some(previous.into_any_element()),
            Some(next.into_any_element()),
        ];
        let (row, gap) = (fit.row, fit.gap);
        let line = || div().h(row).flex().items_center().gap(gap);
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(gap)
            .children(plan.lines.iter().enumerate().map(|(index, parts_of_line)| {
                // What follows Match case on a wrapped line stands at the
                // line's end, after the line's one gap: a spacer between
                // them took a gap either side of it, one more than the plan
                // counts. On one line the field takes the free room.
                let after_case = parts_of_line
                    .iter()
                    .position(|part| *part == Part::Case)
                    .filter(|_| parts_of_line[0] != Part::Field)
                    .map_or(parts_of_line.len(), |at| at + 1);
                let (leading, trailing) = parts_of_line.split_at(after_case);
                let mut take = |parts_here: &[Part]| -> Vec<AnyElement> {
                    parts_here
                        .iter()
                        .filter_map(|part| parts[*part as usize].take())
                        .collect()
                };
                let leading = take(leading);
                let trailing = take(trailing);
                let drawn = line()
                    .debug_selector(move || LINE_SELECTORS.get(index).copied().unwrap_or("").into())
                    .children(leading);
                if trailing.is_empty() {
                    drawn
                } else {
                    drawn.child(line().ml_auto().children(trailing))
                }
            }))
            .into_any_element()
    }
}

/// Match case's checkbox form, labelled, on `focus`.
fn match_case_box(focus: &FocusHandle, sensitive: bool) -> Checkbox {
    Checkbox::new("file-find-match-case")
        .track_focus(focus)
        .debug_selector(|| "file-find-match-case".into())
        .label("Match case")
        .checked(sensitive)
        .flex_shrink_0()
}

/// Match case's "Aa" form, on the checkbox's `focus`: a check box to
/// assistive technology, named Match case and checked as the checkbox is,
/// so a resize that changes its form changes neither its role nor its
/// state (as a toggle button it was reported pressed where the checkbox was
/// checked).
fn match_case_toggle(focus: &FocusHandle, sensitive: bool) -> gpui_kit::component::button::Button {
    crate::button("file-find-match-case", "Aa", "", sensitive)
        .track_focus(focus)
        .debug_selector(|| "file-find-match-case".into())
        .role(gpui_kit::accesskit::Role::CheckBox)
        .toggled(sensitive)
        .tooltip("Match case")
        .accessibility_label("Match case")
        .flex_shrink_0()
}

fn match_style(active: bool, background_available: bool, colors: MatchColors) -> HighlightStyle {
    let background = if active { colors.current } else { colors.other };
    HighlightStyle {
        background_color: background_available.then(|| rgb(background).into()),
        // Syntax styles supply foreground/font properties only. Avoid competing
        // with them in the pinned renderer's unordered property compositor.
        underline: (active || !background_available).then(|| UnderlineStyle {
            color: Some(rgb(colors.underline).into()),
            thickness: px(1.),
            wavy: false,
        }),
        ..Default::default()
    }
}

// Reject an oversized query rather than silently searching a different prefix.
fn bounded_query(value: &str) -> &str {
    if value.len() <= MAX_QUERY_BYTES {
        value
    } else {
        ""
    }
}
// Highlight a bounded consecutive window around the active match. Navigation
// still visits every native match and updates this window without source scans.
fn highlight_window(count: usize, current: usize) -> Range<usize> {
    let start = current
        .saturating_sub(MAX_HIGHLIGHTS / 2)
        .min(count.saturating_sub(MAX_HIGHLIGHTS));
    start..count.min(start + MAX_HIGHLIGHTS)
}

#[cfg(test)]
mod tests {
    use super::{MAX_HIGHLIGHTS, MAX_QUERY_BYTES, bounded_query, highlight_window};
    #[test]
    fn find_highlights_follow_navigation_through_large_match_sets() {
        let count = 100_000;
        for cursor in [0, 1, MAX_HIGHLIGHTS - 1, 50_000, count - 1] {
            let window = highlight_window(count, cursor);
            assert!(window.contains(&cursor));
            assert_eq!(window.len(), MAX_HIGHLIGHTS);
            assert!(window.end <= count);
        }
        assert_eq!(highlight_window(0, 0), 0..0);
        assert_eq!(highlight_window(3, 2), 0..3);
    }
    #[test]
    fn closed_find_engine_keeps_unicode_ranges_case_and_navigation() {
        use gpui_kit::base::input::SearchSession;
        let mut session = SearchSession::default();
        let source = "λ file\r\nFILE λ\nfile".into();
        session.matcher.update(&source);
        session.matcher.update_query("file", true);
        assert!(!session.open);
        assert_eq!(&*session.matcher.matched_ranges(), &[3..7, 9..13, 17..21]);
        assert_eq!(session.matcher.next_back(), Some(17..21));
        // Theme changes and unchanged quiet refreshes do not reset the cursor.
        session.matcher.update(&source);
        assert_eq!(session.matcher.current_match_index(), 2);
        assert_eq!(session.matcher.next(), Some(3..7));
        session.matcher.update_query("file", false);
        assert_eq!(&*session.matcher.matched_ranges(), &[3..7, 17..21]);
    }
    #[test]
    fn find_background_and_active_underline_survive_real_viewport_style_composition() {
        use crate::appearance::ThemeChoice;
        use gpui_kit::{HighlightStyle, combine_highlights, component::input::TextDecoration, rgb};
        for (theme, active) in ThemeChoice::ALL
            .into_iter()
            .flat_map(|theme| [(theme, true), (theme, false)])
        {
            let palette = theme.palette();
            let highlights = palette.editor_highlights();
            let colors = super::MatchColors {
                other: highlights.find_match,
                current: highlights.find_current,
                underline: palette.accent,
            };
            let find = super::match_style(active, true, colors);
            // The current match and the others draw on their own fitted backgrounds.
            assert_eq!(
                find.background_color,
                Some(
                    rgb(if active {
                        highlights.find_current
                    } else {
                        highlights.find_match
                    })
                    .into()
                )
            );
            for unified in [false, true] {
                for (foreground, background) in [
                    (palette.added, palette.added_background),
                    (palette.removed, palette.removed_background),
                ] {
                    let patch = HighlightStyle {
                        background_color: Some(rgb(background).into()),
                        color: unified.then(|| rgb(foreground).into()),
                        ..Default::default()
                    };
                    // Match in the middle of changing visible syntax runs. A
                    // two-style test with the match in the first run missed
                    // GPUI's unordered active-style fold and passed falsely.
                    for visible_runs in 1..64 {
                        let start = (visible_runs / 2) * 10 + 3;
                        let range = start..start + 4;
                        let source = [TextDecoration::new(0..visible_runs * 10, patch)];
                        let masked =
                            super::without_match_backgrounds(&source, std::slice::from_ref(&range));
                        let syntax = (0..visible_runs).map(|index| {
                            (
                                index * 10..index * 10 + 10,
                                HighlightStyle {
                                    color: Some(rgb(palette.text).into()),
                                    ..Default::default()
                                },
                            )
                        });
                        let base = combine_highlights(
                            syntax,
                            masked.into_iter().map(|d| (d.range, d.style)),
                        )
                        .collect::<Vec<_>>();
                        // Check either collection order through the actual
                        // renderer compositor, including its hash-set fold.
                        for styles in [
                            combine_highlights(base.clone(), [(range.clone(), find)])
                                .collect::<Vec<_>>(),
                            combine_highlights([(range.clone(), find)], base.clone())
                                .collect::<Vec<_>>(),
                        ] {
                            let drawn = styles.iter().find(|(r, _)| r.contains(&start)).unwrap().1;
                            assert_eq!(
                                drawn.background_color, find.background_color,
                                "{theme:?}, {active}, {visible_runs}"
                            );
                            assert_eq!(
                                drawn.underline, find.underline,
                                "{theme:?}, {active}, {visible_runs}"
                            );
                            assert!(
                                drawn.color == Some(rgb(palette.text).into())
                                    || drawn.color == patch.color
                            );
                        }
                        assert_eq!(super::without_match_backgrounds(&source, &[]), source);
                    }
                }
            }
        }
    }

    #[test]
    fn match_mask_preserves_styles_across_gaps_boundaries_and_theme_refresh() {
        use gpui_kit::{FontWeight, HighlightStyle, component::input::TextDecoration, rgb};
        let patch = HighlightStyle {
            color: Some(rgb(0xabcdef).into()),
            background_color: Some(rgb(0x123456).into()),
            font_weight: Some(FontWeight::MEDIUM),
            ..Default::default()
        };
        let source = [
            TextDecoration::new(0..12, patch),
            TextDecoration::new(16..26, patch),
        ];
        let matches = [2..4, 6..9, 11..18, 22..26];
        let visible = super::without_match_backgrounds(&source, &matches);
        for offset in 0..26 {
            let original = source.iter().find(|d| d.range.contains(&offset));
            let actual = visible.iter().find(|d| d.range.contains(&offset));
            assert_eq!(original.is_some(), actual.is_some());
            if let (Some(original), Some(actual)) = (original, actual) {
                assert_eq!(actual.style.color, original.style.color);
                assert_eq!(actual.style.font_weight, original.style.font_weight);
                assert_eq!(
                    actual.style.background_color.is_none(),
                    matches.iter().any(|r| r.contains(&offset))
                );
            }
        }
        let changed = [TextDecoration::new(
            0..26,
            HighlightStyle {
                background_color: Some(rgb(0x654321).into()),
                ..patch
            },
        )];
        let themed = super::without_match_backgrounds(&changed, &matches);
        assert_eq!(
            themed[0].style.background_color,
            changed[0].style.background_color
        );
        assert_eq!(super::without_match_backgrounds(&changed, &[]), changed);
        let fallback = super::match_style(
            true,
            false,
            super::MatchColors {
                other: 0,
                current: 0,
                underline: 0xabcdef,
            },
        );
        assert!(fallback.background_color.is_none());
        assert!(fallback.color.is_none());
        assert!(fallback.underline.is_some());
    }

    /// Patch decorations that attach while Find paints matches, as the pull request Source
    /// patch's do when its file's preparation finishes after Find opened, leave no background
    /// under those matches and keep their tint elsewhere.
    #[gpui_kit::test]
    fn decorations_attached_under_open_find_take_no_background_under_its_matches(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use gpui_kit::{
            AppContext as _, HighlightStyle, component::input::EditorState,
            component::input::TextDecoration, rgb,
        };

        cx.update(gpui_kit::init);
        let cx = cx.add_empty_window();
        let text = "keep\nneedle added\nkeep\n";
        let editor = cx.update(|window, cx| {
            let editor = cx.new(|cx| EditorState::new(window, cx).default_value(text));
            super::reserve_highlight_layer(&editor, cx);
            editor.update(cx, |source, cx| {
                source.set_search_query("needle", false, cx)
            });
            let opening = ("File text".into(), None);
            super::show(&editor, editor.entity_id(), opening, None, window, cx);
            editor
        });
        let tint = HighlightStyle {
            color: Some(rgb(0xabcdef).into()),
            background_color: Some(rgb(0x123456).into()),
            ..Default::default()
        };
        let drawn = cx.update(|_, cx| {
            super::patch_decorations(&editor, vec![TextDecoration::new(5..18, tint)], cx).drawn(cx)
        });
        let unmasked = HighlightStyle {
            background_color: None,
            ..tint
        };
        assert_eq!(
            drawn,
            [
                TextDecoration::new(5..11, unmasked),
                TextDecoration::new(11..18, tint)
            ]
        );
    }

    #[test]
    fn find_query_limit_does_not_silently_change_unicode_search() {
        let accepted = "é".repeat(MAX_QUERY_BYTES / 2);
        assert_eq!(bounded_query(&accepted), accepted);
        assert_eq!(bounded_query(&format!("{accepted}x")), "");
        assert_eq!(bounded_query("\r\nλ filename\t"), "\r\nλ filename\t");
    }

    /// Every app editor (patches, files, conflicts and review text) draws code
    /// as typed: the kit editor the wrapper renders gets the code features of
    /// the family it draws in, whichever the desktop supplies, and the bundled
    /// family, which has no ligatures, none. Features a caller chose win.
    #[gpui_kit::test]
    fn editors_draw_code_without_ligatures(cx: &mut gpui_kit::TestAppContext) {
        use gpui_kit::{AppContext as _, Styled as _, component::input::EditorState};

        cx.update(gpui_kit::init);
        let cx = cx.add_empty_window();
        let state = cx.update(|window, cx| cx.new(|cx| EditorState::new(window, cx)));
        let code_family = |family: &'static str, cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|_, cx| super::Theme::global_mut(cx).mono_font_family = family.into())
        };
        // The features on the style of the kit editor that the wrapper renders.
        let rendered = |editor: super::Editor, cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|_, cx| editor.native(cx).style().text.font_features.clone())
        };

        code_family("Desktop Mono", cx);
        let desktop = rendered(super::Editor::new(&state), cx).expect("code features");
        assert_eq!(desktop.is_calt_enabled(), Some(false));
        assert!(desktop.tag_value_list().contains(&("liga".into(), 0)));

        code_family(crate::desktop_text::BUNDLED_CODE_FAMILY, cx);
        let bundled = rendered(super::Editor::new(&state), cx).expect("code features");
        assert!(bundled.tag_value_list().is_empty(), "{bundled:?}");

        // A family the caller refined on the editor is the one it draws in.
        let refined = rendered(super::Editor::new(&state).font_family("Desktop Mono"), cx);
        assert_eq!(
            refined.expect("code features").is_calt_enabled(),
            Some(false)
        );

        let chosen = gpui_kit::FontFeatures(std::sync::Arc::new(vec![("zero".into(), 1)]));
        let kept = rendered(super::Editor::new(&state).font_features(chosen.clone()), cx);
        assert_eq!(kept, Some(chosen));
    }
}

#[cfg(test)]
mod bar_tests {
    use super::{DRAWN_FORMS, FIELD_FLOOR, Forms, panel_height};
    use crate::*;
    use core::prelude::v1::test;

    /// How far apart two drawn edges may read and still meet: layout draws
    /// each on the test window's half pixels.
    const EDGE: Pixels = px(0.5);

    /// The find bar's controls, by debug selector.
    const PARTS: [&str; 5] = [
        "file-find-field",
        "close-file-find",
        "file-find-match-case",
        "previous-file-match",
        "next-file-match",
    ];

    /// Shows Compare's text in `mode` in a `width` × 490 window and opens Find
    /// over its first editor, as Primary+F there does.
    fn open_find(app: &Entity<GitTurtle>, cx: &mut VisualTestContext, mode: TextMode, width: f32) {
        open_find_on(app, cx, mode, 0, width);
    }

    /// [`open_find`] over the editor of Split's `side`, Before's or After's,
    /// or the unified patch's.
    fn open_find_on(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        mode: TextMode,
        side: usize,
        width: f32,
    ) {
        cx.simulate_resize(size(px(width), px(490.)));
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.text_mode = mode;
                app.ensure_editor(window, cx);
                cx.notify();
            })
        });
        crate::width_walk::frame(cx);
        crate::width_walk::frame(cx);
        let editor = cx.update(|_, cx| {
            let app = app.read(cx);
            match mode {
                TextMode::Split => app
                    .split_view
                    .as_ref()
                    .expect("the split view")
                    .read(cx)
                    .editors()[side]
                    .clone(),
                _ => app.patch_editor.clone().expect("the patch editor"),
            }
        });
        let focus = cx.update(|_, cx| editor.focus_handle(cx));
        cx.update(|window, cx| window.focus(&focus, cx));
        cx.dispatch_action(gpui_kit::component::input::Search);
        crate::width_walk::frame(cx);
    }

    /// Where `part` was drawn, which it must be.
    fn drawn(cx: &mut VisualTestContext, part: &'static str, at: &str) -> Bounds<Pixels> {
        cx.debug_bounds(part)
            .unwrap_or_else(|| panic!("{at}: {part} is drawn"))
    }

    /// The find bar keeps each control whole, its focus ring inside the bar,
    /// and the field at least its floor, eight digits of its text inside its
    /// padding and border, from a 1,480 px window down to 461 × 490 at 13 and
    /// 18 pt, in the unified patch and in Split, opened over Before, whose
    /// bar spans both sides (about 177 px at 13 pt in that window, as the
    /// unified patch's). It keeps one line, one
    /// 28 px control's height, while the field's floor and every control fit
    /// it, no taller at 1,480 px than the two lines of 20 and 24 px controls
    /// before (46.5 px at 13 pt, 64 at 18), Previous and Next taking their
    /// icons first, then Match case its "Aa" toggle, then Close its icon;
    /// past that it wraps whole, Previous and Next on one line, and a bar
    /// narrower than the floor gives the field its whole line. At 461 × 490
    /// and 13 pt the unified bar drew its field 24 px wide beside Close and
    /// cut "Match case". Previous and Next name their keys as the platform
    /// does ([`crate::shortcuts::ENTER`]).
    #[gpui::test]
    async fn the_find_bar_keeps_every_control_whole_from_wide_to_narrow(cx: &mut TestAppContext) {
        let (_fixture, app, cx, _) =
            crate::width_walk::open(crate::width_walk::Page::CompareText, cx).await;
        let ring = cx.update(|_, cx| appearance::button_ring_room(cx));
        let mut met = Vec::new();
        for text in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            for (name, mode) in [("unified", TextMode::Unified), ("split", TextMode::Split)] {
                let mut wider: Option<Forms> = None;
                for width in [1480., 1200., 1000., 800., 640., 540., 461.] {
                    let at = format!("{text} pt, {name}, {width} × 490");
                    open_find(&app, cx, mode, width);
                    let bar = drawn(cx, "file-find-bar", &at);
                    let (forms, _, lines) = DRAWN_FORMS
                        .with_borrow(|drawn| *drawn)
                        .unwrap_or_else(|| panic!("{at}: the controls are laid out"));
                    let before = if text == 13 { px(46.5) } else { px(64.) };
                    if width == 1480. {
                        assert!(
                            lines == 1 && bar.size.height <= before + EDGE,
                            "{at}: the bar {bar:?} keeps one line, {lines} drawn, no taller \
                             than {before:?}"
                        );
                    }
                    for (part, key) in [
                        ("previous-file-match", shortcuts::shifted(shortcuts::ENTER)),
                        ("next-file-match", shortcuts::ENTER.to_owned()),
                    ] {
                        let control = crate::system_preview::DRAWN_CONTROLS
                            .with_borrow(|drawn| drawn.get(part).cloned())
                            .unwrap_or_else(|| panic!("{at}: {part} is drawn"));
                        assert!(
                            control.name.ends_with(&format!(", {key}")),
                            "{at}: {part} is named {:?}, naming {key:?}",
                            control.name
                        );
                    }
                    let (_, floor, case) = cx.update(|window, _| {
                        let text = |text: &str, size: Pixels| {
                            crate::views::laid_out_text_width(
                                crate::views::line_widths(&[(text, size)], window),
                                window,
                            )
                        };
                        let rem = window.rem_size();
                        (
                            rem,
                            text(FIELD_FLOOR, rem * 0.875) + px(18.),
                            rem * 1.5 + text("Match case", rem),
                        )
                    });
                    // Inside the bar's pane inset either side, and above its
                    // 1 px bottom rule.
                    let room = bar.size.width - appearance::ui_size(appearance::PANE_INSET) * 2.;
                    let parts: Vec<_> = PARTS
                        .iter()
                        .map(|part| (*part, drawn(cx, part, &at)))
                        .collect();
                    let (previous, next) = (parts[3].1, parts[4].1);
                    assert!(
                        (previous.top() - next.top()).abs() <= EDGE,
                        "{at}: Previous {previous:?} and Next {next:?} share a line"
                    );
                    for (part, bounds) in &parts {
                        assert!(
                            bounds.left() - ring >= bar.left() - EDGE
                                && bounds.right() + ring <= bar.right() + EDGE
                                && bounds.top() - ring >= bar.top() - EDGE
                                && bounds.bottom() + ring <= bar.bottom() - px(1.) + EDGE,
                            "{at}: {part} {bounds:?} lies whole with its {ring:?} ring inside \
                             the bar {bar:?}"
                        );
                    }
                    for (index, (part, bounds)) in parts.iter().enumerate() {
                        for (other, against) in &parts[index + 1..] {
                            assert!(
                                bounds.right() <= against.left() + EDGE
                                    || against.right() <= bounds.left() + EDGE
                                    || bounds.bottom() <= against.top() + EDGE
                                    || against.bottom() <= bounds.top() + EDGE,
                                "{at}: {part} {bounds:?} runs into {other} {against:?}"
                            );
                        }
                    }
                    let field = parts[0].1;
                    assert!(
                        field.size.width >= floor.min(room) - EDGE,
                        "{at}: the field {field:?} keeps its {floor:?} floor, or the bar's \
                         whole {room:?} where that is less"
                    );
                    if forms.case {
                        let drawn = parts[2].1;
                        assert!(
                            drawn.size.width >= case - EDGE,
                            "{at}: Match case {drawn:?} draws its box and its whole {case:?} label"
                        );
                    }
                    // A narrower window only takes labels away.
                    if let Some(wider) = wider {
                        assert!(
                            (wider.close || !forms.close)
                                && (wider.pair || !forms.pair)
                                && (wider.case || !forms.case),
                            "{at}: {forms:?} gives back a label {wider:?} gave up"
                        );
                    }
                    wider = Some(forms);
                    met.push(forms);
                    cx.simulate_keystrokes("escape");
                    crate::width_walk::frame(cx);
                }
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        for forms in [
            Forms {
                close: true,
                pair: true,
                case: true,
            },
            Forms {
                close: false,
                pair: false,
                case: false,
            },
        ] {
            assert!(met.contains(&forms), "the walk meets {forms:?}: {met:?}");
        }
    }

    /// The find bar's Close, Previous and Next paint their whole labels at
    /// fractional scales: at scales 1.1, 1.2, 1.25, 1.5 and 1.75 and every
    /// interface size from 11 to 18 pt, in the unified patch and Split's
    /// first side, each paints its whole label between the insets it draws
    /// either side of it, and is at least as wide as layout places that
    /// label in its frame (`crate::placed_button_width`). Each size meets
    /// the scales in turn, so a width the bar kept from another scale's
    /// frame is drawn too. Sized as the unrounded sum of their insets and
    /// labels rounded up to a device pixel, 24 of the 120 controls each
    /// mode draws here were a device pixel narrower than that, at 13 of the
    /// 40 sizes and scales (Close 78.33 px where layout places it at 79.17
    /// at 1.2 and 18 pt);
    /// the tests' font painted those labels whole, inside their insets, in
    /// the rounding to a whole pixel their widths leave, which the desktop's
    /// fonts need not leave.
    #[gpui::test]
    async fn the_find_bar_paints_its_labels_whole_at_fractional_scales(cx: &mut TestAppContext) {
        use crate::system_preview::DRAWN_CONTROLS;
        let (_fixture, app, cx, _) =
            crate::width_walk::open(crate::width_walk::Page::CompareText, cx).await;
        let mark = gpui_kit::Background::from(crate::width_walk::text_mark());
        let mut failures = Vec::new();
        for (name, mode) in [("unified", TextMode::Unified), ("split", TextMode::Split)] {
            open_find(&app, cx, mode, 1800.);
            for text in 11..=18 {
                for scale in [1.1f32, 1.2, 1.25, 1.5, 1.75] {
                    cx.update(|window, cx| {
                        window.set_scale_factor(scale);
                        appearance::apply_text_sizes(text, 12, window, cx)
                    });
                    DRAWN_CONTROLS.with_borrow_mut(|drawn| drawn.clear());
                    crate::width_walk::frame(cx);
                    let at = format!("{name}, scale {scale}, {text} pt");
                    for part in ["close-file-find", "previous-file-match", "next-file-match"] {
                        let control = DRAWN_CONTROLS
                            .with_borrow(|drawn| drawn.get(part).cloned())
                            .unwrap_or_else(|| panic!("{at}: {part} is drawn"));
                        assert_eq!(control.form, "labelled", "{at}: {part} is labelled");
                        let bounds = drawn(cx, part, &at);
                        let label = control.shows;
                        let (runs, whole, inset, placed) = cx.update(|window, _| {
                            let scale = window.scale_factor();
                            let runs: Vec<_> = window
                                .painted_quads()
                                .iter()
                                .filter(|quad| quad.background == mark)
                                .map(|quad| {
                                    let left = px(quad.bounds.left().0 / scale);
                                    let width = px(quad.bounds.size.width.0 / scale);
                                    (left, width, px(quad.bounds.top().0 / scale))
                                })
                                .filter(|(left, width, top)| {
                                    bounds.contains(&point(*left + *width / 2., *top + px(1.)))
                                })
                                .map(|(left, width, _)| (left, width))
                                .collect();
                            let size = appearance::ui_text(12.);
                            let weight = window.text_style().font_weight;
                            let whole = crate::views::shaped_width(&label, size, weight, window);
                            let text = crate::views::line_widths(&[(&label, size)], window);
                            (
                                runs,
                                whole,
                                window.pixel_snap(appearance::ui_size(crate::LABEL_INSET)),
                                crate::placed_button_width(text, false, window),
                            )
                        });
                        let painted = runs.iter().fold(Pixels::ZERO, |sum, run| sum + run.1);
                        if painted < whole - px(0.5) {
                            failures.push(format!(
                                "{at}: {part} reads {label:?} cut, {painted:?} of {whole:?}"
                            ));
                        }
                        // The label's painted run, between the insets the
                        // control draws either side of it: drawn narrower
                        // than its label, its run reaches into the inset,
                        // where the control's frame clips it.
                        let left = runs.iter().map(|run| run.0).fold(Pixels::MAX, Pixels::min);
                        let right = runs
                            .iter()
                            .map(|run| run.0 + run.1)
                            .fold(Pixels::MIN, Pixels::max);
                        if left < bounds.left() + inset - px(0.01)
                            || right > bounds.right() - inset + px(0.01)
                        {
                            failures.push(format!(
                                "{at}: {part}'s {label:?} is painted from {left:?} to {right:?}, \
                                 into its {inset:?} insets in {bounds:?}"
                            ));
                        }
                        if bounds.size.width < placed - px(0.01) {
                            failures.push(format!(
                                "{at}: {part} is {:?} wide, short of the {placed:?} layout \
                                 places {label:?} at",
                                bounds.size.width
                            ));
                        }
                    }
                }
            }
            cx.simulate_keystrokes("escape");
            crate::width_walk::frame(cx);
        }
        cx.update(|window, cx| {
            window.set_scale_factor(2.);
            appearance::apply_text_sizes(13, 12, window, cx)
        });
        assert!(
            failures.is_empty(),
            "{} labels cut or short:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }

    /// The find bar's plan sums the gaps layout draws between its parts: in
    /// the unified patch at 13 and 18 pt and display scales 1.2 and 1.25,
    /// in the narrowest window, to the device pixel, that keeps each form
    /// the bar draws on one line, the line the plan chose that form by (the
    /// field's floor, the count's reserved room, each control and the gaps
    /// between them) is as wide as the line layout draws there, the field
    /// counted at its floor and the count at its room, every control and
    /// gap as drawn; and the line lies inside the bar's inset. The plan
    /// keeps the form as far as the room drawn holds it: a device pixel
    /// narrower, the room inside the inset is under the line, so the field,
    /// which takes the room left on the line, is drawn at its floor, with
    /// what the count leaves of its room, and less than that device pixel's
    /// room more. Summed from
    /// the raw quarter rem, 3.25 px at 13 pt, the plan's five gaps were
    /// 0.42 px short of the 4 device pixels layout draws each at 1.2 and
    /// 0.25 px over them at 1.25; and Match case's box and gap, rounded up
    /// together, made its checkbox a device pixel wider than drawn at 1.25.
    #[gpui::test]
    async fn the_find_bar_plans_its_forms_with_the_gaps_layout_draws(cx: &mut TestAppContext) {
        let (_fixture, app, cx, _) =
            crate::width_walk::open(crate::width_walk::Page::CompareText, cx).await;
        let mut failures = Vec::new();
        let mut thresholds = Vec::new();
        for scale in [1.2f32, 1.25] {
            for text in [13, 18] {
                cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
                open_find(&app, cx, TextMode::Unified, 1480.);
                // Windows are counted in device pixels.
                let width = |n: i32| n as f32 / scale;
                let forms_at = |n: i32, cx: &mut VisualTestContext| {
                    cx.simulate_resize(size(px(width(n)), px(490.)));
                    at_scale(cx, scale);
                    DRAWN_FORMS
                        .with_borrow(|drawn| *drawn)
                        .map(|(forms, _, lines)| (forms, lines))
                        .expect("the controls are laid out")
                };
                let (narrow, wide) = ((461. * scale) as i32, (1480. * scale) as i32);
                let step = (16. * scale) as i32;
                let mut wider = (wide, forms_at(wide, cx));
                let mut n = wide - step;
                while n >= narrow {
                    let here = forms_at(n, cx);
                    if here != wider.1 {
                        // The narrowest window that keeps the wider form.
                        let (mut lo, mut hi) = (n, wider.0);
                        while hi - lo > 1 {
                            let middle = lo + (hi - lo) / 2;
                            if forms_at(middle, cx) == wider.1 {
                                hi = middle;
                            } else {
                                lo = middle;
                            }
                        }
                        // The room inside the bar's inset a device pixel
                        // narrower, where the bar gives the form up.
                        forms_at(hi - 1, cx);
                        let inset = cx.update(|window, _| {
                            window.pixel_snap(appearance::ui_size(appearance::PANE_INSET))
                        });
                        let narrower = cx
                            .debug_bounds("file-find-bar")
                            .expect("the bar is drawn")
                            .size
                            .width
                            - inset * 2.;
                        let (forms, lines) = forms_at(hi, cx);
                        if lines == 1 {
                            let at =
                                format!("scale {scale}, {text} pt, {forms:?} in {}", width(hi));
                            let widths = super::DRAWN_WIDTHS
                                .with_borrow(|drawn| *drawn)
                                .expect("the controls are laid out");
                            let line = [
                                "file-find-field",
                                "file-find-match-case",
                                "file-find-count",
                                "previous-file-match",
                                "next-file-match",
                                "close-file-find",
                            ]
                            .map(|part| drawn(cx, part, &at));
                            let gaps = line
                                .windows(2)
                                .map(|pair| pair[1].left() - pair[0].right())
                                .fold(Pixels::ZERO, |sum, gap| sum + gap);
                            let [field, case, _, previous, next, close] = line;
                            let laid_out = widths.floor
                                + case.size.width
                                + widths.count
                                + previous.size.width
                                + next.size.width
                                + close.size.width
                                + gaps;
                            let planned = widths.line(forms);
                            let bar = drawn(cx, "file-find-bar", &at);
                            thresholds.push(format!(
                                "{at}: {planned:?}, drawn {laid_out:?}, {narrower:?} a device \
                                 pixel narrower"
                            ));
                            // The plan keeps the form wherever the room
                            // drawn holds it, so the field takes beyond its
                            // floor, with what the count leaves of its room,
                            // only what that device pixel gave up.
                            if (planned - laid_out).abs() > px(0.01)
                                || field.size.width < widths.floor - px(0.01)
                                || narrower >= planned - px(0.01)
                                || close.right() > bar.right() - inset + px(0.01)
                            {
                                failures.push(format!(
                                    "{at}: the plan's line is {planned:?} and layout draws \
                                     {laid_out:?} with gaps of {gaps:?}; the field \
                                     {field:?} keeps its {:?} floor; a device pixel \
                                     narrower the room is {narrower:?}, under the line; and \
                                     Close {close:?} ends inside the bar {bar:?}'s \
                                     {inset:?} inset",
                                    widths.floor
                                ));
                            }
                        }
                    }
                    wider = (n, here);
                    n -= step;
                }
                cx.simulate_keystrokes("escape");
                crate::width_walk::frame(cx);
            }
        }
        cx.update(|window, cx| {
            window.set_scale_factor(2.);
            appearance::apply_text_sizes(13, 12, window, cx)
        });
        eprintln!("thresholds:\n{}", thresholds.join("\n"));
        assert!(
            thresholds.len() >= 8,
            "the walk meets the one-line forms' changes: {thresholds:?}"
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// Pastes `text` into the focused field, as Primary+V does, in one edit.
    fn paste(cx: &mut VisualTestContext, text: &str) {
        let item = gpui_kit::ClipboardItem::new_string(text.to_owned());
        cx.update(|_, cx| cx.write_to_clipboard(item));
        cx.dispatch_action(gpui_kit::component::input::Paste);
        crate::width_walk::frame(cx);
    }

    /// The carets the last frame painted, in the window's pixels: quads in
    /// the kit's caret colour, as wide as its caret.
    fn painted_carets(cx: &mut VisualTestContext) -> Vec<Bounds<Pixels>> {
        let caret =
            cx.read(|cx| gpui_kit::Background::from(gpui_kit::component::Theme::global(cx).caret));
        cx.update(|window, _| {
            let scale = window.scale_factor();
            let width = if cfg!(target_os = "macos") { 1.5 } else { 2. } * scale;
            window
                .painted_quads()
                .into_iter()
                .filter(|quad| {
                    quad.background == caret
                        && (quad.bounds.size.width.as_f32() - width).abs() < 0.01
                })
                .map(|quad| {
                    let bounds = quad.bounds;
                    Bounds::new(
                        point(
                            px(bounds.origin.x.as_f32() / scale),
                            px(bounds.origin.y.as_f32() / scale),
                        ),
                        size(
                            px(bounds.size.width.as_f32() / scale),
                            px(bounds.size.height.as_f32() / scale),
                        ),
                    )
                })
                .collect()
        })
    }

    /// Find reopened over a query wider than its field selects the whole
    /// query and shows it from its start, its caret there inside the field,
    /// in the first frame it draws, at 13 and 18 pt, as a macOS find field
    /// shows its query when Command-F returns to it: here the query was typed
    /// in a 1,480 px window, its field following the caret to the query's
    /// end, and Find closed and reopened in a 640 px one. Selected from its
    /// start, the query kept the view it closed with, which the kit does not
    /// follow a whole selection out of, and the caret, at the query's end,
    /// stood past the narrower field's end.
    #[gpui::test]
    async fn find_reopened_over_a_wide_query_shows_it_selected_from_its_start(
        cx: &mut TestAppContext,
    ) {
        let (_fixture, app, cx, _) =
            crate::width_walk::open(crate::width_walk::Page::CompareText, cx).await;
        // The caret is drawn only in the active window.
        cx.update(|window, _| window.activate_window());
        let query = "zq ".repeat(100);
        for text in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            let at = format!("{text} pt, Find reopened in 640 × 490");
            open_find(&app, cx, TextMode::Unified, 1480.);
            paste(cx, &query);
            cx.simulate_keystrokes("escape");
            crate::width_walk::frame(cx);
            cx.simulate_resize(size(px(640.), px(490.)));
            crate::width_walk::frame(cx);
            cx.dispatch_action(gpui_kit::component::input::Search);
            crate::width_walk::frame(cx);
            let field = drawn(cx, "file-find-field", &at);
            let wide = cx.update(|window, _| {
                crate::views::laid_out_text_width(
                    crate::views::line_widths(
                        &[(query.as_str(), window.rem_size() * 0.875)],
                        window,
                    ),
                    window,
                )
            });
            assert!(
                wide > field.size.width,
                "{at}: the query, {wide:?} wide, overflows the field {field:?}"
            );
            let carets = painted_carets(cx);
            assert!(
                carets.len() == 1
                    && carets[0].left() >= field.left()
                    && carets[0].right() <= field.right(),
                "{at}: one caret is drawn, inside the field {field:?}: {carets:?}"
            );
            let editor = cx.update(|_, cx| app.read(cx).patch_editor.clone().expect("the editor"));
            let selected = cx.update(|_, cx| {
                let bar = super::panel(&editor, cx).expect("the find bar");
                let query = bar.read(cx).query.read(cx);
                (query.selected_range(), query.cursor())
            });
            assert_eq!(
                selected,
                (0..query.len(), 0),
                "{at}: the whole query is selected, the caret at its start"
            );
            assert!(
                carets[0].right() <= field.left() + field.size.width / 2.,
                "{at}: the caret {:?} stands at the field {field:?}'s start",
                carets[0]
            );
            cx.simulate_keystrokes("escape");
            crate::width_walk::frame(cx);
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The find bar's bounds, its parts' and how many lines they are drawn
    /// on: a line's parts stand on its middle, lines a control apart.
    fn drawn_lines(
        cx: &mut VisualTestContext,
        at: &str,
    ) -> (Bounds<Pixels>, Vec<Bounds<Pixels>>, usize) {
        let bar = drawn(cx, "file-find-bar", at);
        let parts: Vec<_> = PARTS
            .iter()
            .chain(&["file-find-count"])
            .map(|part| drawn(cx, part, at))
            .collect();
        let mut middles: Vec<f32> = parts.iter().map(|part| part.center().y.into()).collect();
        middles.sort_by(f32::total_cmp);
        let half = f32::from(appearance::ui_size(14.));
        middles.dedup_by(|below, above| *below - *above < half);
        (bar, parts, middles.len())
    }

    /// The find bar's height is the lines its controls are drawn on, in
    /// Split at 13 and 18 pt, opened over Before, in the
    /// windows a device pixel apart around each width where the bar's lines
    /// change, which the walk finds by bisection: the bar's last line ends as
    /// far above its 1 px rule as the first starts below its top. Layout
    /// measures the bar at its unrounded width and draws it on device pixels,
    /// rounding each edge, and the bar took its height from the first and its
    /// lines from the second: on Before's side at 13 pt in a 572.5 × 490
    /// window at the test window's scale, 2, when each side drew a bar of its
    /// own, it was three lines tall and drew four, the last below its rule,
    /// over the code. Across both sides now, the bar changes its lines once
    /// between the walk's ends, from two to one, and is the same bar opened
    /// over either side.
    #[gpui::test]
    async fn the_find_bar_is_as_tall_as_the_lines_it_draws(cx: &mut TestAppContext) {
        let (_fixture, app, cx, _) =
            crate::width_walk::open(crate::width_walk::Page::CompareText, cx).await;
        let scale = cx.update(|window, _| window.scale_factor());
        // Windows are counted in device pixels.
        let width = |n: i32| n as f32 / scale;
        let (narrow, wide) = ((461. * scale) as i32, (1480. * scale) as i32);
        let mut changes = 0;
        for text in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            open_find_on(&app, cx, TextMode::Split, 0, width(wide));
            // The lines drawn in a window `n` device pixels wide, whose
            // bar must be as tall as they are.
            let lines = |n: i32, cx: &mut VisualTestContext| {
                let at = format!("{text} pt, Before's bar in {} × 490", width(n));
                cx.simulate_resize(size(px(width(n)), px(490.)));
                crate::width_walk::frame(cx);
                let (bar, parts, lines) = drawn_lines(cx, &at);
                let top = parts
                    .iter()
                    .map(|part| part.top())
                    .fold(bar.bottom(), Pixels::min);
                let bottom = parts
                    .iter()
                    .map(|part| part.bottom())
                    .fold(bar.top(), Pixels::max);
                // Its padding above the first line and above its rule
                // below the last, which lines of a control's height, a
                // shorter control centred on one, cannot tell apart from
                // a missing line or an empty one.
                let (above, below) = (top - bar.top(), bar.bottom() - px(1.) - bottom);
                assert!(
                    below >= -EDGE && (above - below).abs() < appearance::ui_size(14.),
                    "{at}: the bar {bar:?} draws {lines} lines, its parts from {above:?} \
                     below its top to {below:?} above its rule"
                );
                lines
            };
            // Walked 16 px at a time, then bisected where the lines change.
            let step = (16. * scale) as i32;
            let mut wider = (narrow, lines(narrow, cx));
            let mut n = narrow + step;
            while n <= wide {
                let here = lines(n, cx);
                if here != wider.1 {
                    let (mut lo, mut hi) = (wider.0, n);
                    while hi - lo > 1 {
                        let middle = lo + (hi - lo) / 2;
                        if lines(middle, cx) == here {
                            hi = middle;
                        } else {
                            lo = middle;
                        }
                    }
                    for n in hi - 3..=hi + 3 {
                        lines(n, cx);
                    }
                    changes += 1;
                }
                wider = (n, here);
                n += step;
            }
            cx.simulate_keystrokes("escape");
            crate::width_walk::frame(cx);
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        assert!(
            changes >= 2,
            "the walk meets the bar's lines changing at both sizes: {changes}"
        );
    }

    /// In Split with Find open on Before, the two sides' first rows stand
    /// level in every frame drawn after a resize that changes the bar's
    /// lines, the first among them, at 13 and 18 pt: both sides' text starts
    /// at the foot of the one bar drawn across them. When each side drew a
    /// bar of its own and After's was padded a frame later from the height
    /// Before's had measured, After's rows stood a line apart from Before's
    /// in the first frame after the resize.
    #[gpui::test]
    async fn split_rows_stay_level_in_the_frame_a_resize_changes_the_find_bar(
        cx: &mut TestAppContext,
    ) {
        use crate::diff_view::painted_rows;
        let (_fixture, app, cx, _) =
            crate::width_walk::open(crate::width_walk::Page::CompareText, cx).await;
        for text in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            open_find(&app, cx, TextMode::Split, 1480.);
            let sides = cx.update(|_, cx| {
                let split = app.read(cx).split_view.clone().expect("the split view");
                split
                    .read(cx)
                    .editors()
                    .clone()
                    .map(|editor| editor.entity_id())
            });
            let mut lines = std::collections::BTreeSet::new();
            painted_rows::watch();
            for width in [461., 1480., 640., 1480.] {
                let at = format!("{text} pt, resized to {width} × 490");
                painted_rows::take();
                cx.simulate_resize(size(px(width), px(490.)));
                crate::views::tests::draw(cx);
                let painted = painted_rows::take();
                lines.extend(DRAWN_FORMS.with_borrow(|drawn| drawn.map(|drawn| drawn.2)));
                assert!(!painted.is_empty(), "{at}: the sides are painted");
                // Each frame paints Before's rows, then After's.
                for (index, frame) in painted.chunks(2).enumerate() {
                    let [(before, before_top), (after, after_top)] = frame else {
                        panic!("{at}: frame {index} paints both sides: {frame:?}");
                    };
                    assert_eq!([*before, *after], sides, "{at}: frame {index}'s sides");
                    assert!(
                        before_top.is_some() && before_top == after_top,
                        "{at}: in frame {index} of {}, Before's first row stands at \
                         {before_top:?} and After's at {after_top:?}",
                        painted.len() / 2
                    );
                }
            }
            painted_rows::stop();
            assert!(
                lines.len() > 1,
                "{text} pt: the resizes change the bar's lines: {lines:?}"
            );
            cx.simulate_keystrokes("escape");
            crate::width_walk::frame(cx);
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Split's sides share one Find bar, drawn across both in their
    /// headings' row, at 13 and 18 pt: opened over Before's text it spans
    /// both sides, the headings giving it their row; opened
    /// over After's it closes Before's, so one bar stands there; Primary+F in
    /// its field keeps it open, the field focused, and selects its whole
    /// query again from wherever its caret stood; Escape in Before's text
    /// closes After's bar, focus staying in that text, and the headings
    /// return; and the sides' first rows stand level in every frame, through
    /// resizes that change the bar's lines. On a side of its own each bar
    /// took four lines in a 461 × 490 window and left no row in view.
    #[gpui::test]
    async fn split_sides_share_one_find_bar_across_both(cx: &mut TestAppContext) {
        use crate::diff_view::painted_rows;
        let (_fixture, app, cx, _) =
            crate::width_walk::open(crate::width_walk::Page::CompareText, cx).await;
        // The caret is drawn only in the active window.
        cx.update(|window, _| window.activate_window());
        // Each frame paints Before's rows, then After's, level.
        let level = |at: &str, sides: [gpui_kit::EntityId; 2]| {
            let painted = painted_rows::take();
            assert!(!painted.is_empty(), "{at}: the sides are painted");
            for (index, frame) in painted.chunks(2).enumerate() {
                let [(before, before_top), (after, after_top)] = frame else {
                    panic!("{at}: frame {index} paints both sides: {frame:?}");
                };
                assert_eq!([*before, *after], sides, "{at}: frame {index}'s sides");
                assert!(
                    before_top.is_some() && before_top == after_top,
                    "{at}: in frame {index} of {}, Before's first row stands at \
                     {before_top:?} and After's at {after_top:?}",
                    painted.len() / 2
                );
            }
        };
        for text in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            open_find_on(&app, cx, TextMode::Split, 0, 1480.);
            let editors = cx.update(|_, cx| {
                let split = app.read(cx).split_view.clone().expect("the split view");
                split.read(cx).editors().clone()
            });
            let sides = editors.clone().map(|editor| editor.entity_id());
            let open = |cx: &mut VisualTestContext| {
                cx.update(|_, cx| {
                    editors.clone().map(|editor| {
                        super::panel(&editor, cx).is_some_and(|bar| bar.read(cx).open)
                    })
                })
            };
            // The bar spans both sides, standing on them.
            let spans = |at: &str, cx: &mut VisualTestContext| {
                let bar = drawn(cx, "file-find-bar", at);
                let before = drawn(cx, "split-text-0", at);
                let after = drawn(cx, "split-text-1", at);
                assert!(
                    (bar.left() - before.left()).abs() <= EDGE
                        && (bar.right() - after.right()).abs() <= px(1.) + EDGE
                        && (bar.bottom() - before.top()).abs() <= EDGE
                        && (bar.bottom() - after.top()).abs() <= EDGE,
                    "{at}: the bar {bar:?} spans Before {before:?} and After {after:?}, \
                     standing on both"
                );
            };
            let at = format!("{text} pt, Find opened over Before");
            assert_eq!(open(cx), [true, false], "{at}: Before's bar is open");
            spans(&at, cx);
            open_find_on(&app, cx, TextMode::Split, 1, 1480.);
            let at = format!("{text} pt, Find opened over After");
            assert_eq!(
                open(cx),
                [false, true],
                "{at}: After's bar replaces Before's"
            );
            spans(&at, cx);
            // Primary+F in the field, its caret at the end of a query wider
            // than the field, which follows the caret there, selects the
            // whole query again and shows it from its start, as from the
            // text: the bar, which stands outside the text's element, takes
            // the key itself.
            let query = "zq ".repeat(100);
            paste(cx, &query);
            cx.simulate_keystrokes("end");
            crate::width_walk::frame(cx);
            let field = drawn(cx, "file-find-field", &at);
            let reading = |cx: &mut VisualTestContext| {
                cx.update(|window, cx| {
                    let bar = super::panel(&editors[1], cx).expect("After's bar");
                    let query = bar.read(cx).query.read(cx);
                    (
                        query.selected_range(),
                        query.cursor(),
                        query.focus_handle(cx).is_focused(window),
                    )
                })
            };
            assert_eq!(
                reading(cx),
                (query.len()..query.len(), query.len(), true),
                "{at}: the caret stands at the query's end"
            );
            let carets = painted_carets(cx);
            assert!(
                carets.len() == 1 && carets[0].left() >= field.left() + field.size.width / 2.,
                "{at}: the field shows the query's end, its caret {carets:?} there"
            );
            cx.dispatch_action(gpui_kit::component::input::Search);
            crate::width_walk::frame(cx);
            assert!(
                reading(cx) == (0..query.len(), 0, true) && open(cx) == [false, true],
                "{text} pt: Primary+F in the field keeps After's bar open, its field focused, \
                 its whole query selected, the caret at its start: {:?}",
                reading(cx)
            );
            let carets = painted_carets(cx);
            assert!(
                carets.len() == 1
                    && carets[0].left() >= field.left()
                    && carets[0].right() <= field.left() + field.size.width / 2.,
                "{text} pt: the field shows the query from its start, its caret {carets:?} \
                 there, in {field:?}"
            );
            painted_rows::watch();
            for width in [461., 1480., 640., 1480.] {
                cx.simulate_resize(size(px(width), px(490.)));
                crate::views::tests::draw(cx);
                level(&format!("{text} pt, resized to {width} × 490"), sides);
            }
            // Escape in Before's text closes the bar After opened.
            let before_focus = cx.update(|_, cx| editors[0].focus_handle(cx));
            cx.update(|window, cx| window.focus(&before_focus, cx));
            painted_rows::take();
            cx.simulate_keystrokes("escape");
            crate::views::tests::draw(cx);
            level(&format!("{text} pt, as the bar closes"), sides);
            painted_rows::stop();
            assert_eq!(open(cx), [false, false], "{text} pt: Escape closes the bar");
            assert!(
                cx.update(|window, _| before_focus.is_focused(window)),
                "{text} pt: focus stays in Before's text"
            );
            assert!(
                cx.debug_bounds("file-find-bar").is_none(),
                "{text} pt: no bar is drawn"
            );
            let heights =
                cx.update(|_, cx| editors.clone().map(|editor| panel_height(&editor, cx)));
            assert_eq!(heights, [px(0.); 2], "{text} pt: both bars closed");
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The editors of Split's sides, Before's and After's.
    fn split_editors(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
    ) -> [Entity<EditorState>; 2] {
        cx.update(|_, cx| {
            let split = app.read(cx).split_view.clone().expect("the split view");
            split.read(cx).editors().clone()
        })
    }

    /// What a frame painted on a find bar's rule, in device pixels.
    struct PaintedRule {
        /// Where the border colour runs across the bar's foot.
        rule: Option<Bounds<ScaledPixels>>,
        /// What the accent colour paints on the bar's foot.
        marks: Vec<Mark>,
    }

    /// The accent on a bar's rule: where it is drawn, and what the frame
    /// painted over it after it.
    #[derive(Debug)]
    struct Mark {
        drawn: Bounds<ScaledPixels>,
        covered: Vec<Bounds<ScaledPixels>>,
    }

    /// The rule the bar drawn at `bar` paints across its foot in `border`,
    /// and what `accent` paints there, as `window`'s last frame painted them
    /// in its order: a quad paints its bounds within its mask, which narrows
    /// a border's quads to their strips, over every quad before it.
    fn painted_rule(
        window: &Window,
        bar: Bounds<Pixels>,
        [accent, border]: [gpui_kit::Hsla; 2],
    ) -> PaintedRule {
        let scale = window.scale_factor();
        let quads = window.painted_quads();
        // What a quad may paint: its bounds within its mask, which narrows a
        // border's quads to their strips and a device pixel inside them.
        let area = |quad: &gpui_kit::Quad| quad.bounds.intersect(&quad.content_mask.bounds);
        // The rows it paints at its foot: a fill's whole area, a border's
        // bottom edge.
        let foot = |quad: &gpui_kit::Quad| {
            let bounds = quad.bounds;
            let edge = point(bounds.left(), bounds.bottom() - quad.border_widths.bottom);
            let drawn = if quad.background.is_transparent() {
                Bounds::from_corners(edge, bounds.bottom_right())
            } else {
                bounds
            };
            drawn.intersect(&quad.content_mask.bounds)
        };
        // A fill's colour, or a border's where the quad has no fill.
        let colour = |quad: &gpui_kit::Quad| {
            if quad.background.is_transparent() {
                Some(quad.border_color)
            } else {
                quad.background.as_solid()
            }
        };
        let bar = bar.scale(scale);
        // Within a logical pixel of the bar's foot, no taller than two.
        let at_foot = |drawn: &Bounds<ScaledPixels>| {
            !drawn.is_empty()
                && (drawn.bottom().as_f32() - bar.bottom().as_f32()).abs() <= scale
                && drawn.size.height.as_f32() <= scale * 2.
                && drawn.left().as_f32() >= bar.left().as_f32() - scale
                && drawn.right().as_f32() <= bar.right().as_f32() + scale
        };
        let rule = quads
            .iter()
            .map(|quad| (colour(quad), foot(quad)))
            .find(|(colour, drawn)| {
                *colour == Some(border)
                    && at_foot(drawn)
                    && drawn.size.width.as_f32() >= bar.size.width.as_f32() - scale
            })
            .map(|(_, drawn)| drawn);
        let marks = quads
            .iter()
            .enumerate()
            .filter(|(_, quad)| colour(quad) == Some(accent) && at_foot(&foot(quad)))
            .map(|(at, quad)| {
                let drawn = foot(quad);
                let covered = quads[at + 1..]
                    .iter()
                    .map(area)
                    .filter(|over| over.intersects(&drawn))
                    .collect();
                Mark { drawn, covered }
            })
            .collect();
        PaintedRule { rule, marks }
    }

    /// The shared bar marks the side it searches in every state: that
    /// side's half of its 1 px rule is painted in the accent colour, the
    /// other half not, opened over Before and over After, with a query that
    /// matches nothing, in 461 and 1,480 px windows at display scales 2 and
    /// 1.2, on the rule's own device pixel rows inside the bar with nothing
    /// painted over it (a child of the bar, it lay under the bar's rule,
    /// which a box paints after its children, and never showed natively);
    /// and its empty field
    /// names that side whole, "Find in Before" or, where that does not fit
    /// the field, "Before", as at the width where the bar has just become
    /// one line, its field near its floor, reporting the whole form to
    /// assistive technology there. With a query typed, the headings hidden,
    /// nothing said which side the bar searched, and "Find in Before" was
    /// cut at that width.
    #[gpui::test]
    async fn the_shared_bar_marks_and_names_the_side_it_searches(cx: &mut TestAppContext) {
        let (_fixture, app, cx, _) =
            crate::width_walk::open(crate::width_walk::Page::CompareText, cx).await;
        cx.update(|_, cx| appearance::ThemeChoice::Midnight.apply(None, cx));
        let colours = cx.update(|_, cx| {
            let palette = appearance::palette(cx);
            [palette.accent, palette.border].map(|colour| gpui_kit::Hsla::from(rgb(colour)))
        });
        open_find_on(&app, cx, TextMode::Split, 0, 1480.);
        let editors = split_editors(&app, cx);
        cx.simulate_keystrokes("escape");
        crate::width_walk::frame(cx);
        for scale in [2., 1.2] {
            for (side, name) in [(0, "Before"), (1, "After")] {
                for width in [461., 1480.] {
                    let at = format!("scale {scale}, Find over {name} in {width} × 490");
                    open_find_on(&app, cx, TextMode::Split, side, width);
                    // A resize takes the test display's scale, 2.
                    cx.update(|window, _| window.set_scale_factor(scale));
                    paste(cx, "no such text");
                    crate::views::tests::draw(cx);
                    let bar = drawn(cx, "file-find-bar", &at);
                    let sides = ["split-text-0", "split-text-1"].map(|text| drawn(cx, text, &at));
                    let empty = cx.update(|_, cx| {
                        let bar = super::panel(&editors[side], cx).expect("the bar");
                        bar.read(cx).reading(cx).2
                    });
                    assert!(empty, "{at}: the query matches nothing");
                    let PaintedRule { rule, marks } =
                        cx.update(|window, _| painted_rule(window, bar, colours));
                    let rule = rule.unwrap_or_else(|| panic!("{at}: the bar {bar:?} draws a rule"));
                    let [mark] = marks.as_slice() else {
                        panic!("{at}: one accent mark on the rule {rule:?}: {marks:?}");
                    };
                    // In device pixels, as painted.
                    let (searched, bar) = (sides[side].scale(scale), bar.scale(scale));
                    let edge = EDGE.as_f32() * scale;
                    let f = ScaledPixels::as_f32;
                    assert!(
                        mark.drawn.top() == rule.top() && mark.drawn.bottom() == rule.bottom(),
                        "{at}: the mark {mark:?} is drawn on the rule's rows, {rule:?}"
                    );
                    assert!(
                        f(mark.drawn.top()) >= f(bar.top()) - edge
                            && f(mark.drawn.bottom()) <= f(bar.bottom()) + edge
                            && (f(mark.drawn.left()) - f(searched.left())).abs() <= edge
                            && (f(mark.drawn.right()) - f(searched.right())).abs() <= scale + edge,
                        "{at}: the mark {mark:?} lies inside the bar {bar:?}, under {name}'s \
                         side {searched:?} alone"
                    );
                    assert!(
                        mark.covered.is_empty(),
                        "{at}: nothing is painted over the mark {mark:?}"
                    );
                    cx.simulate_keystrokes("escape");
                    crate::width_walk::frame(cx);
                }
            }
        }
        cx.update(|window, _| window.set_scale_factor(2.));
        // The width where the bar has just become one line, a device pixel
        // past the widest of two, found by bisection.
        // Reopened, the field selects its query, which a key clears.
        open_find_on(&app, cx, TextMode::Split, 0, 1480.);
        cx.simulate_keystrokes("backspace");
        crate::width_walk::frame(cx);
        let scale = cx.update(|window, _| window.scale_factor());
        let lines = |n: i32, cx: &mut VisualTestContext| {
            cx.simulate_resize(size(px(n as f32 / scale), px(490.)));
            crate::width_walk::frame(cx);
            DRAWN_FORMS.with_borrow(|drawn| drawn.expect("the controls are laid out").2)
        };
        let (mut lo, mut hi) = ((461. * scale) as i32, (1480. * scale) as i32);
        assert!(
            lines(lo, cx) == 2 && lines(hi, cx) == 1,
            "the bar takes two lines, then one"
        );
        while hi - lo > 1 {
            let middle = lo + (hi - lo) / 2;
            if lines(middle, cx) == 1 {
                hi = middle;
            } else {
                lo = middle;
            }
        }
        for n in [lo, hi] {
            lines(n, cx);
            let at = format!("Find over Before in {} × 490", n as f32 / scale);
            let (shown, whole, room, wide) = cx.update(|window, cx| {
                let bar = super::panel(&editors[0], cx).expect("Before's bar");
                let query = bar.read(cx).query.read(cx);
                let presented = query.presentation();
                let shown = presented.placeholder().clone();
                let room = query
                    .text_bounds()
                    .expect("the field is laid out")
                    .size
                    .width;
                let weight = window.text_style().font_weight;
                let wide =
                    crate::views::shaped_width(&shown, window.rem_size() * 0.875, weight, window);
                (
                    shown,
                    presented.accessible_placeholder().clone(),
                    room,
                    wide,
                )
            });
            assert!(
                ["Find in Before", "Before"].contains(&shown.as_ref()) && wide <= room + px(0.01),
                "{at}: the field shows {shown:?}, {wide:?} wide, whole in its {room:?}"
            );
            assert_eq!(
                whole.as_ref(),
                "Find in Before",
                "{at}: its accessible placeholder"
            );
        }
        cx.simulate_keystrokes("escape");
        crate::width_walk::frame(cx);
    }

    /// Split's bar is one bar with one query: opened over After's text while
    /// it searches Before, it moves there with its query and Match case,
    /// searches After with them, its field showing the query, and Before's
    /// matches clear. Opened anew over After, its bar took After's own last
    /// query, here none, so a search typed over Before was lost.
    #[gpui::test]
    async fn split_s_bar_moves_with_its_query_and_match_case(cx: &mut TestAppContext) {
        let (_fixture, app, cx, _) =
            crate::width_walk::open(crate::width_walk::Page::CompareText, cx).await;
        open_find_on(&app, cx, TextMode::Split, 0, 1480.);
        let editors = split_editors(&app, cx);
        paste(cx, "Line");
        cx.update(|_, cx| {
            let bar = super::panel(&editors[0], cx).expect("Before's bar");
            bar.update(cx, |bar, cx| bar.match_case(true, cx));
        });
        crate::width_walk::frame(cx);
        let state = |side: usize, cx: &mut VisualTestContext| {
            cx.update(|_, cx| {
                let bar = super::panel(&editors[side], cx).expect("the bar");
                let bar = bar.read(cx);
                let session = editors[side].read(cx).search_session();
                (
                    bar.open,
                    bar.query.read(cx).value().to_string(),
                    session.query.clone(),
                    !session.case_insensitive,
                    bar.painted
                        .as_ref()
                        .map_or(0, |painted| painted.ranges.len()),
                )
            })
        };
        let (open, field, query, sensitive, matches) = state(0, cx);
        assert!(
            open && field == "Line" && query == "Line" && sensitive && matches > 0,
            "Before's bar searches it for Line, matching case: {:?}",
            state(0, cx)
        );
        open_find_on(&app, cx, TextMode::Split, 1, 1480.);
        let (open, field, query, sensitive, matches) = state(1, cx);
        assert!(
            open && field == "Line" && query == "Line" && sensitive && matches > 0,
            "the bar over After shows Line, matching case, and finds it there: {:?}",
            state(1, cx)
        );
        let (open, _, _, _, matches) = state(0, cx);
        assert!(
            !open && matches == 0,
            "Before's bar is closed, its matches cleared"
        );
        let lowered = cx.update(|_, cx| {
            let bar = super::panel(&editors[1], cx).expect("After's bar");
            bar.read(cx).reading(cx).0
        });
        assert!(lowered, "Match case shows checked over After");
        cx.simulate_keystrokes("escape");
        crate::width_walk::frame(cx);
    }

    /// Tab from the shared bar's last control goes to the text it searches,
    /// After's while it searches After, at 13 pt in a 461 × 490 window, where
    /// Next ends the bar's second line; and Shift+Tab from that text returns
    /// to that control. Before's text follows the bar in the window's order,
    /// so Tab from it went there, and the read-only text took Shift+Tab for
    /// outdenting and kept focus.
    #[gpui::test]
    async fn tab_leaves_split_s_bar_for_the_text_it_searches_and_returns(cx: &mut TestAppContext) {
        let (_fixture, app, cx, _) =
            crate::width_walk::open(crate::width_walk::Page::CompareText, cx).await;
        open_find_on(&app, cx, TextMode::Split, 1, 461.);
        let editors = split_editors(&app, cx);
        paste(cx, "Line");
        let (bar, after) = cx.update(|_, cx| {
            let bar = super::panel(&editors[1], cx).expect("After's bar");
            (bar.read(cx).focus.clone(), editors[1].focus_handle(cx))
        });
        // Tab through the bar's controls to the last.
        let mut last = None;
        for _ in 0..8 {
            if !cx.update(|window, cx| bar.contains_focused(window, cx)) {
                break;
            }
            last = cx.update(|window, cx| window.focused(cx));
            cx.simulate_keystrokes("tab");
            crate::width_walk::frame(cx);
        }
        let last = last.expect("the bar holds focus");
        assert!(
            cx.update(|window, _| after.is_focused(window)),
            "Tab from the bar's last control goes to After's text, which it searches"
        );
        cx.simulate_keystrokes("shift-tab");
        crate::width_walk::frame(cx);
        assert!(
            cx.update(|window, _| last.is_focused(window)),
            "Shift+Tab from After's text returns to the bar's last control"
        );
        cx.simulate_keystrokes("escape");
        crate::width_walk::frame(cx);
    }

    /// Escape with any of the find bar's controls focused, Match case,
    /// Previous, Next or Close, closes Find as Escape in its field does:
    /// focus returns to the text it searches and Compare stays, in the
    /// unified patch's bar of its own and in Split's shared bar over either
    /// side, on one line and on two. Off the field the key is the app's
    /// Escape, Back from Compare, so Escape on Next left Compare for History.
    #[gpui::test]
    async fn escape_on_any_find_bar_control_closes_find_and_stays_in_compare(
        cx: &mut TestAppContext,
    ) {
        let (_fixture, app, cx, _) =
            crate::width_walk::open(crate::width_walk::Page::CompareText, cx).await;
        // The app's keys, as the window binds them: off the field Escape is
        // the app's, which the test window binds to nothing else.
        cx.update(|_, cx| crate::shortcuts::bind_keys(cx));
        for (mode, side, width, name) in [
            (TextMode::Split, 0, 461., "Split's bar over Before"),
            (TextMode::Split, 1, 1480., "Split's bar over After"),
            (TextMode::Unified, 0, 1480., "the unified patch's bar"),
        ] {
            let (mut stops, mut case) = (Vec::new(), false);
            // The bar's four controls follow its field in Tab order.
            for tabs in 1..=4 {
                let at = format!("{name} in {width} × 490, Tab {tabs} from its field");
                open_find_on(&app, cx, mode, side, width);
                paste(cx, "Line");
                let (editor, bar) = cx.update(|_, cx| {
                    let app = app.read(cx);
                    let editor = match mode {
                        TextMode::Split => app
                            .split_view
                            .as_ref()
                            .expect("the split view")
                            .read(cx)
                            .editors()[side]
                            .clone(),
                        _ => app.patch_editor.clone().expect("the patch editor"),
                    };
                    let bar = super::panel(&editor, cx).expect("the bar");
                    (editor, bar)
                });
                let (within, field, case_focus) = cx.update(|_, cx| {
                    let bar = bar.read(cx);
                    let field = bar.query.read(cx).focus_handle(cx);
                    (bar.focus.clone(), field, bar.case_focus.clone())
                });
                for _ in 0..tabs {
                    cx.simulate_keystrokes("tab");
                    crate::width_walk::frame(cx);
                }
                let focused = cx
                    .update(|window, cx| window.focused(cx))
                    .unwrap_or_else(|| panic!("{at}: something holds focus"));
                assert!(
                    cx.update(|window, cx| within.contains_focused(window, cx)) && focused != field,
                    "{at}: one of the bar's controls holds focus"
                );
                assert!(
                    !stops.contains(&focused),
                    "{at}: Tab moves on to a control not focused before"
                );
                case |= focused == case_focus;
                stops.push(focused);
                cx.simulate_keystrokes("escape");
                crate::width_walk::frame(cx);
                let (open, compare, back) = cx.update(|window, cx| {
                    (
                        bar.read(cx).open,
                        app.read(cx).mode == WorkspaceMode::Compare,
                        editor.focus_handle(cx).is_focused(window),
                    )
                });
                assert!(
                    !open && compare && back,
                    "{at}: Escape closes Find ({}), Compare stays shown ({compare}) and focus \
                     returns to the text Find searched ({back})",
                    !open
                );
            }
            assert!(case, "{name}: Match case is one of the controls");
        }
    }

    /// What Split shows with Find open over Before in the window as it is,
    /// checked: the bar's lines, at most two; every control and the count
    /// whole inside the bar with its focus ring, and the bar above Compare's
    /// footer, or the preview's foot where the footer gave way; both sides'
    /// first rows level; and the rows each side shows whole between the
    /// bar's foot and the footer, at least three, which it returns with the
    /// lines and whether the footer is drawn.
    fn split_find_holds(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        at: &str,
    ) -> (usize, usize, bool) {
        let ring = cx.update(|_, cx| appearance::button_ring_room(cx));
        let (bar, parts, lines) = drawn_lines(cx, at);
        let planned = DRAWN_FORMS
            .with_borrow(|drawn| *drawn)
            .unwrap_or_else(|| panic!("{at}: the controls are laid out"))
            .2;
        assert!(
            lines <= 2 && planned == lines,
            "{at}: the bar {bar:?} draws {lines} lines, planned {planned}, at most two"
        );
        let footer = cx.debug_bounds("compare-footer");
        // The footer's top, or the preview's foot where it gave way.
        let foot = match footer {
            Some(footer) => footer.top(),
            None => drawn(cx, "preview-content", at).bottom(),
        };
        assert!(
            bar.bottom() <= foot + EDGE,
            "{at}: the bar {bar:?} stands above the footer {footer:?}, or the preview's \
             foot at {foot:?}"
        );
        for (part, bounds) in PARTS.iter().chain(&["file-find-count"]).zip(&parts) {
            let ring = if *part == "file-find-count" {
                px(0.)
            } else {
                ring
            };
            assert!(
                bounds.left() - ring >= bar.left() - EDGE
                    && bounds.right() + ring <= bar.right() + EDGE
                    && bounds.top() - ring >= bar.top() - EDGE
                    && bounds.bottom() + ring <= bar.bottom() - px(1.) + EDGE,
                "{at}: {part} {bounds:?} lies whole with its {ring:?} ring inside the bar \
                 {bar:?}, above the footer {footer:?}"
            );
        }
        let editors = cx.update(|_, cx| {
            let split = app.read(cx).split_view.clone().expect("the split view");
            split.read(cx).editors().clone()
        });
        let shown: Vec<_> = cx.update(|_, cx| {
            editors
                .iter()
                .map(|editor| {
                    let editor = editor.read(cx);
                    let viewport = editor.text_bounds().expect("the text is laid out");
                    let line = editor.line_height().expect("the text is laid out");
                    let first = viewport.top() + editor.scroll_offset().y;
                    // The rows standing whole in the text's viewport.
                    let whole = (0..editor.value().lines().count())
                        .filter(|row| {
                            let top = first + line * *row as f32;
                            top >= viewport.top() - EDGE && top + line <= viewport.bottom() + EDGE
                        })
                        .count();
                    (first, viewport, whole)
                })
                .collect()
        });
        let [
            (before, before_view, before_rows),
            (after, after_view, after_rows),
        ] = [shown[0], shown[1]];
        assert!(
            before == after,
            "{at}: Before's first row stands at {before:?} and After's at {after:?}"
        );
        for (side, view) in [("Before", before_view), ("After", after_view)] {
            assert!(
                view.top() >= bar.bottom() - EDGE && view.bottom() <= foot + EDGE,
                "{at}: {side}'s text {view:?} lies between the bar {bar:?} and the footer \
                 {footer:?}, or the preview's foot at {foot:?}"
            );
        }
        let whole = before_rows.min(after_rows);
        assert!(
            whole >= 3,
            "{at}: Before shows {before_rows} rows whole and After {after_rows}, at least \
             three, the footer {footer:?}: {before_view:?}, {after_view:?}"
        );
        (lines, whole, footer.is_some())
    }

    /// Draws the window at display `scale` again after a resize, which takes
    /// the test display's own scale, 2: set before it, a scale of 1.2 was
    /// lost, and the walks below ran at scale 2 alone.
    fn at_scale(cx: &mut VisualTestContext, scale: f32) {
        cx.update(|window, _| window.set_scale_factor(scale));
        crate::width_walk::frame(cx);
    }

    /// Compare's footer, drawn whole beneath the preview: as Find closes, or
    /// in a window that leaves Split's text its rows beside it.
    fn footer_is_back(cx: &mut VisualTestContext, at: &str) {
        let footer = drawn(cx, "compare-footer", at);
        let content = drawn(cx, "preview-content", at);
        assert!(
            (footer.top() - content.bottom()).abs() <= EDGE,
            "{at}: the footer {footer:?} is back beneath the preview {content:?}"
        );
    }

    /// In Split in a 461 × 490 window, at 13 and 18 pt and display scales 2
    /// and 1.2, Find's bar, open over Before and across both sides, takes at
    /// most two lines, keeps every control whole inside it and above
    /// Compare's footer, and leaves both sides' rows level and three whole
    /// in view: at 18 pt Compare's footer gives way to them, where the bar's
    /// two lines, 92.5 px, left two whole rows and the top of a third above
    /// the footer, and comes back as Find closes, its words kept in the
    /// comparison's accessible description while it is away. On a side of
    /// its own the bar took four lines, 129 px at 13 pt and 179 at 18, its
    /// last cut by the footer, and no row of either side was in view.
    #[gpui::test]
    async fn split_find_bar_in_a_461_by_490_window_keeps_its_lines_controls_and_rows(
        cx: &mut TestAppContext,
    ) {
        let (_fixture, app, cx, _) =
            crate::width_walk::open(crate::width_walk::Page::CompareText, cx).await;
        let mut met = Vec::new();
        for scale in [2., 1.2] {
            for text in [13, 18] {
                cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
                crate::views::COMPACT_LABELS.with_borrow_mut(|labels| {
                    labels.remove("split-comparison");
                });
                open_find(&app, cx, TextMode::Split, 461.);
                at_scale(cx, scale);
                let at = format!("scale {scale}, {text} pt, Split in 461 × 490");
                let bar = drawn(cx, "file-find-bar", &at);
                let (lines, whole, footer) = split_find_holds(&app, cx, &at);
                if !footer {
                    let line = crate::views::COMPACT_LABELS
                        .with_borrow(|labels| labels.get("split-comparison").cloned());
                    assert!(
                        line.as_ref().is_some_and(|line| line.contains(" → ")),
                        "{at}: the comparison is described by the footer's words: {line:?}"
                    );
                }
                met.push((scale, text, bar.size.height, lines, whole, footer));
                cx.simulate_keystrokes("escape");
                crate::width_walk::frame(cx);
                footer_is_back(cx, &format!("{at}, Find closed"));
            }
        }
        cx.update(|window, cx| {
            window.set_scale_factor(2.);
            appearance::apply_text_sizes(13, 12, window, cx);
        });
        eprintln!("scale, size, bar height, its lines, whole rows and the footer: {met:?}");
        assert!(
            met.iter()
                .any(|(_, text, _, _, _, footer)| *text == 18 && !footer),
            "the footer gives way at 18 pt: {met:?}"
        );
    }

    /// The same, walked from a 461 px window to a 1,000 px one 12 px at a
    /// time at 13 and 18 pt and scales 2 and 1.2, Find open over Before: no
    /// width between draws the bar on more than two lines, cuts a control,
    /// stands the sides' rows apart or shows fewer than three rows; and
    /// Compare's footer is back in a 1,480 px window and as Find closes.
    #[gpui::test]
    async fn split_find_bar_holds_from_461_to_1000_px(cx: &mut TestAppContext) {
        let (_fixture, app, cx, _) =
            crate::width_walk::open(crate::width_walk::Page::CompareText, cx).await;
        for scale in [2., 1.2] {
            for text in [13, 18] {
                cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
                open_find(&app, cx, TextMode::Split, 461.);
                let mut width = 461.;
                while width <= 1000. {
                    cx.simulate_resize(size(px(width), px(490.)));
                    at_scale(cx, scale);
                    let at = format!("scale {scale}, {text} pt, Split in {width} × 490");
                    split_find_holds(&app, cx, &at);
                    width += 12.;
                }
                cx.simulate_resize(size(px(1480.), px(490.)));
                at_scale(cx, scale);
                let at = format!("scale {scale}, {text} pt, Split in 1480 × 490");
                split_find_holds(&app, cx, &at);
                footer_is_back(cx, &at);
                cx.simulate_resize(size(px(461.), px(490.)));
                at_scale(cx, scale);
                cx.simulate_keystrokes("escape");
                crate::width_walk::frame(cx);
                let at = format!("scale {scale}, {text} pt, Find closed in 461 × 490");
                footer_is_back(cx, &at);
            }
        }
        cx.update(|window, cx| {
            window.set_scale_factor(2.);
            appearance::apply_text_sizes(13, 12, window, cx);
        });
    }

    /// Split's footer rule reckons the open Find bar as tall as layout draws
    /// it (`views::split_room`, through [`super::shared_bar_height`]): at 13
    /// and 18 pt and display scales 1.2 and 2, Find open over Before, in
    /// windows from 461 to 1,480 px wide and in the windows a device pixel
    /// either side of each width where the bar's lines change, which the
    /// walk finds by bisection as
    /// `the_find_bar_is_as_tall_as_the_lines_it_draws` does, the height the
    /// rule reckoned with in the frame is the drawn bar's: its lines planned
    /// from the room inside its insets as layout draws them, and its lines,
    /// padding and rule each on the device pixels layout draws them on.
    /// Summed from the raw quarter rem and 1 px, the bar's two lines at
    /// 13 pt in a 461 × 490 window were reckoned at 66.75 px and drawn
    /// 67.5 px at 1.2 and 66.5 at 2.
    #[gpui::test]
    async fn split_s_footer_rule_reckons_the_find_bar_as_layout_draws_it(cx: &mut TestAppContext) {
        let (_fixture, app, cx, _) =
            crate::width_walk::open(crate::width_walk::Page::CompareText, cx).await;
        cx.update(|_, cx| cx.set_reduce_motion(true));
        let mut failures = Vec::new();
        let mut changes = Vec::new();
        for scale in [1.2f32, 2.] {
            // Windows are counted in device pixels.
            let width = |n: i32| n as f32 / scale;
            let (narrow, wide) = ((461. * scale).ceil() as i32, (1480. * scale) as i32);
            for text in [13, 18] {
                cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
                open_find(&app, cx, TextMode::Split, width(narrow));
                // The lines the bar draws in a window `n` device pixels
                // wide, where the rule must reckon it as tall as drawn.
                let mut lines = |n: i32, cx: &mut VisualTestContext| {
                    let at = format!("scale {scale}, {text} pt, Split in {} × 490", width(n));
                    cx.simulate_resize(size(px(width(n)), px(490.)));
                    crate::views::SPLIT_FIND_BAR.set(None);
                    at_scale(cx, scale);
                    let bar = drawn(cx, "file-find-bar", &at);
                    let lines = DRAWN_FORMS
                        .with_borrow(|drawn| *drawn)
                        .unwrap_or_else(|| panic!("{at}: the controls are laid out"))
                        .2;
                    let reckoned = crate::views::SPLIT_FIND_BAR
                        .get()
                        .unwrap_or_else(|| panic!("{at}: the footer's rule reckons the bar"));
                    if (reckoned - bar.size.height).abs() > px(0.01) {
                        failures.push(format!(
                            "{at}: the rule reckons the bar's {lines} lines {reckoned:?} tall, \
                             drawn {:?}",
                            bar.size.height
                        ));
                    }
                    lines
                };
                // Walked 16 px at a time, then bisected where the lines
                // change, and read on both sides of the change.
                let step = (16. * scale) as i32;
                let mut wider = (narrow, lines(narrow, cx));
                let mut n = narrow + step;
                while n <= wide {
                    let here = lines(n, cx);
                    if here != wider.1 {
                        let (mut lo, mut hi) = (wider.0, n);
                        while hi - lo > 1 {
                            let middle = lo + (hi - lo) / 2;
                            if lines(middle, cx) == here {
                                hi = middle;
                            } else {
                                lo = middle;
                            }
                        }
                        let sides = (lines(lo, cx), lines(hi, cx));
                        changes.push((scale, text, width(hi), sides));
                    }
                    wider = (n, here);
                    n += step;
                }
                cx.simulate_keystrokes("escape");
                crate::width_walk::frame(cx);
            }
        }
        cx.update(|window, cx| {
            window.set_scale_factor(2.);
            appearance::apply_text_sizes(13, 12, window, cx);
        });
        eprintln!("scale, size, the wider window and the lines either side: {changes:?}");
        for scale in [1.2, 2.] {
            for text in [13, 18] {
                assert!(
                    changes
                        .iter()
                        .any(|change| change.0 == scale && change.1 == text),
                    "the walk meets the bar's lines changing at scale {scale} and {text} pt: \
                     {changes:?}"
                );
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// Split's footer stays beneath an open Find bar only while it leaves
    /// each side three whole rows of its text and a device pixel more
    /// (`views::split_room`): at display scales 1.1, 1.2 and 2 and at 13 and
    /// 18 pt, Find open over Before in a window 1,480 px wide, in the
    /// shortest window that keeps the footer, found by bisection over device
    /// pixels, each side's text has that room in view. The rule reckoned the
    /// editor's padding above and below its lines at the raw 8 px, which
    /// layout draws on 10 device pixels at 1.2, 8.33 px each, and on 9 at
    /// 1.1: at 1.2 and 18 pt the footer stayed with each side's text
    /// 54.17 px tall, three 18 px rows and 0.17 px, a fifth of the device
    /// pixel the rule keeps for the text's edges.
    #[gpui::test]
    async fn split_s_footer_stays_only_with_three_rows_and_a_device_pixel_to_spare(
        cx: &mut TestAppContext,
    ) {
        let (_fixture, app, cx, _) =
            crate::width_walk::open(crate::width_walk::Page::CompareText, cx).await;
        let mut failures = Vec::new();
        for scale in [1.1f32, 1.2, 2.] {
            // Windows are counted in device pixels.
            let height = |n: i32| n as f32 / scale;
            for text in [13, 18] {
                cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
                open_find(&app, cx, TextMode::Split, 1480.);
                let editors = split_editors(&app, cx);
                let kept = |n: i32, cx: &mut VisualTestContext| {
                    cx.simulate_resize(size(px(1480.), px(height(n))));
                    at_scale(cx, scale);
                    cx.debug_bounds("compare-footer").is_some()
                };
                let (mut lo, mut hi) = ((200. * scale) as i32, (490. * scale) as i32);
                assert!(
                    !kept(lo, cx) && kept(hi, cx),
                    "scale {scale}, {text} pt: the footer gives way in a window {} px tall \
                     and stays in one 490 px tall",
                    height(lo)
                );
                while hi - lo > 1 {
                    let middle = lo + (hi - lo) / 2;
                    if kept(middle, cx) {
                        hi = middle;
                    } else {
                        lo = middle;
                    }
                }
                kept(hi, cx);
                let at = format!("scale {scale}, {text} pt, Split in 1480 × {}", height(hi));
                for (side, editor) in ["Before", "After"].into_iter().zip(&editors) {
                    let (viewport, line) = cx.update(|_, cx| {
                        let editor = editor.read(cx);
                        (
                            editor.text_bounds().expect("the text is laid out"),
                            editor.line_height().expect("the text is laid out"),
                        )
                    });
                    let room = line * 3. + px(1. / scale);
                    if viewport.size.height < room - px(1e-3) {
                        failures.push(format!(
                            "{at}: {side}'s text {viewport:?} shows less than three {line:?} \
                             rows and a device pixel, {room:?}, above the footer"
                        ));
                    }
                }
                cx.simulate_resize(size(px(1480.), px(490.)));
                at_scale(cx, scale);
                cx.simulate_keystrokes("escape");
                crate::width_walk::frame(cx);
            }
        }
        cx.update(|window, cx| {
            window.set_scale_factor(2.);
            appearance::apply_text_sizes(13, 12, window, cx);
        });
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// A wrapped bar keeps on its line what fills the line's room exactly at
    /// a fractional scale ([`super::FIT_SLACK`]): at 1.2, Match case's "Aa"
    /// toggle, the count and Previous and Next's icons, 40, 30 and 34 device
    /// pixels with their three gaps of 4, fill the 150 device pixels of the
    /// bar's room inside its insets, where the sum of their lengths in
    /// pixels runs a float's error past the room's 124.99999 px. Compared
    /// without the slack, Previous and Next took a third line, a control's
    /// height more.
    #[test]
    fn a_wrapped_find_bar_keeps_what_fills_its_line_exactly_at_a_fractional_scale() {
        use super::{Part, Widths, planned_width};
        let scale = 1.2f32;
        let device = |n: f32| px(n / scale);
        let widths = Widths {
            floor: device(100.),
            close: device(80.),
            previous: device(100.),
            next: device(70.),
            case: device(150.),
            toggle: device(40.),
            count: device(30.),
            icon: device(34.),
            gap: device(4.),
        };
        // The room inside the bar's insets, on the whole device pixel at or
        // below the width layout measures.
        let room = planned_width(device(150.3), scale);
        // The second line, summed as the plan sums its parts.
        let second = widths.toggle
            + (widths.gap + widths.count)
            + widths.gap
            + (widths.gap + widths.icon + widths.icon);
        assert!(
            second > room && second - room < px(1e-4),
            "the second line, {second:?}, runs a float's error past the room, {room:?}"
        );
        assert_eq!(
            widths.plan(room).lines,
            vec![
                vec![Part::Field, Part::Close],
                vec![Part::Case, Part::Count, Part::Previous, Part::Next],
            ],
            "Previous and Next end the line they fill in a room of {room:?}"
        );
    }

    /// Split's footer rule reckons the bar with the limit's notice as tall
    /// as layout draws it (`FindBar::height_at`): with a query past Find's
    /// 4 KiB limit typed, at display scales 1, 1.2, 1.25, 1.5 and 2 and at
    /// 13 and 18 pt, Find open over Before in 461 and 1,480 px windows, the
    /// bar draws the notice below its controls, and the height the rule
    /// reckoned with is the drawn bar's: the gap before the notice on its
    /// whole device pixel, and the notice wrapped at the bar's width inside
    /// its insets, each line on its device pixel. Reckoned as one line of a
    /// whole pixel, the notice was 36 px short at 13 pt in the 461 px window
    /// at scale 1, where it wraps on three, and a device pixel over at 1.2
    /// and 1.25 in the wide one.
    #[gpui::test]
    async fn split_s_footer_rule_reckons_the_find_bar_with_its_limit_notice(
        cx: &mut TestAppContext,
    ) {
        let (_fixture, app, cx, _) =
            crate::width_walk::open(crate::width_walk::Page::CompareText, cx).await;
        let query = "z".repeat(super::MAX_QUERY_BYTES + 1);
        let mut failures = Vec::new();
        for scale in [1f32, 1.2, 1.25, 1.5, 2.] {
            for text in [13, 18] {
                cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
                for width in [461., 1480.] {
                    let at = format!("scale {scale}, {text} pt, Split in {width} × 490");
                    open_find(&app, cx, TextMode::Split, width);
                    paste(cx, &query);
                    crate::views::SPLIT_FIND_BAR.set(None);
                    at_scale(cx, scale);
                    let bar = drawn(cx, "file-find-bar", &at);
                    let lines = DRAWN_FORMS
                        .with_borrow(|drawn| *drawn)
                        .unwrap_or_else(|| panic!("{at}: the controls are laid out"))
                        .2;
                    let last = drawn(cx, super::LINE_SELECTORS[lines - 1], &at);
                    assert!(
                        bar.bottom() - last.bottom() > appearance::ui_text(11.),
                        "{at}: the bar {bar:?} states the limit below its last line {last:?}"
                    );
                    let reckoned = crate::views::SPLIT_FIND_BAR
                        .get()
                        .unwrap_or_else(|| panic!("{at}: the footer's rule reckons the bar"));
                    if (reckoned - bar.size.height).abs() > px(0.01) {
                        failures.push(format!(
                            "{at}: the rule reckons the bar with its notice {reckoned:?} tall, \
                             drawn {:?}",
                            bar.size.height
                        ));
                    }
                    cx.simulate_keystrokes("escape");
                    crate::width_walk::frame(cx);
                }
            }
        }
        cx.update(|window, cx| {
            window.set_scale_factor(2.);
            appearance::apply_text_sizes(13, 12, window, cx);
        });
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// Opens Find in each case the bar's device-pixel tests walk and hands
    /// `check` the case, its display scale, its mode, the bar drawn and the
    /// rule it paints, in Midnight's colours: at display scales 1, 1.2,
    /// 1.25, 1.5 and 2 and at 13 and 18 pt, in the unified patch and over
    /// each of Split's sides, in 461 and 1,480 px windows. Returns the lines
    /// each case's bar drew, which meet one and two.
    fn each_drawn_bar(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        mut check: impl FnMut(&mut VisualTestContext, &str, f32, TextMode, Bounds<Pixels>, PaintedRule),
    ) -> Vec<usize> {
        cx.update(|_, cx| appearance::ThemeChoice::Midnight.apply(None, cx));
        let colours = cx.update(|_, cx| {
            let palette = appearance::palette(cx);
            [palette.accent, palette.border].map(|colour| gpui_kit::Hsla::from(rgb(colour)))
        });
        let mut met = Vec::new();
        for scale in [1f32, 1.2, 1.25, 1.5, 2.] {
            for text in [13, 18] {
                cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
                for (name, mode, side) in [
                    ("the unified patch", TextMode::Unified, 0),
                    ("Split over Before", TextMode::Split, 0),
                    ("Split over After", TextMode::Split, 1),
                ] {
                    for width in [461., 1480.] {
                        let at = format!("scale {scale}, {text} pt, {name} in {width} × 490");
                        open_find_on(app, cx, mode, side, width);
                        at_scale(cx, scale);
                        let bar = drawn(cx, "file-find-bar", &at);
                        let lines = DRAWN_FORMS
                            .with_borrow(|drawn| *drawn)
                            .unwrap_or_else(|| panic!("{at}: the controls are laid out"))
                            .2;
                        let painted = cx.update(|window, _| painted_rule(window, bar, colours));
                        check(cx, &at, scale, mode, bar, painted);
                        met.push(lines);
                        cx.simulate_keystrokes("escape");
                        crate::width_walk::frame(cx);
                    }
                }
            }
        }
        cx.update(|window, cx| {
            window.set_scale_factor(2.);
            appearance::apply_text_sizes(13, 12, window, cx);
        });
        for lines in [1, 2] {
            assert!(
                met.contains(&lines),
                "the walk meets a bar of {lines} lines: {met:?}"
            );
        }
        met
    }

    /// The find bar's rule and Split's accent mark on it stand on whole
    /// device pixels in every case [`each_drawn_bar`] walks: the bar's box
    /// and the mark's are whole device pixels from the window's top, the
    /// mark's box ending at the bar's foot; the rule is painted on whole
    /// device rows at the bar's foot, at least one; and the mark on the
    /// rule's rows. Layout rounds each box's edges and each quad it paints
    /// to device pixels, so this holds by construction: the test is a
    /// regression pin for a rule or mark painted outside layout, as a canvas
    /// or an offset prepaint would paint it. QA-94 F1's rule and mark spread
    /// over two device rows natively at scale 1.2 on Hyprland, about 59 and
    /// 40 %, were the compositor's offset of the whole surface, which spread
    /// every other border in the window the same way (under XWayland at
    /// 1.25, one X pixel to a device pixel, the rule is one row).
    #[gpui::test]
    async fn the_find_bar_s_rule_and_mark_stand_on_whole_device_pixels(cx: &mut TestAppContext) {
        let (_fixture, app, cx, _) =
            crate::width_walk::open(crate::width_walk::Page::CompareText, cx).await;
        let whole = |length: f32| (length - length.round()).abs() < 1e-3;
        let f = ScaledPixels::as_f32;
        let mut failures = Vec::new();
        each_drawn_bar(&app, cx, |cx, at, scale, mode, bar, painted| {
            let device = |length: Pixels| f32::from(length) * scale;
            let PaintedRule { rule, marks } = painted;
            let rule = rule.unwrap_or_else(|| panic!("{at}: the bar {bar:?} draws a rule"));
            if !(whole(device(bar.top()))
                && whole(device(bar.bottom()))
                && whole(f(rule.top()))
                && whole(f(rule.size.height))
                && f(rule.size.height) > 1. - 1e-3
                && (f(rule.bottom()) - device(bar.bottom())).abs() < 1e-3)
            {
                failures.push(format!(
                    "{at}: the bar {bar:?} and its rule {rule:?} stand on whole device \
                     pixels, the rule at least one at the bar's foot"
                ));
            }
            match (mode, marks.as_slice()) {
                (TextMode::Split, [mark]) => {
                    let side = drawn(cx, "file-find-side", at);
                    if !(whole(device(side.top()))
                        && whole(device(side.size.height))
                        && (device(side.bottom()) - device(bar.bottom())).abs() < 1e-3
                        && mark.drawn.top() == rule.top()
                        && mark.drawn.bottom() == rule.bottom())
                    {
                        failures.push(format!(
                            "{at}: the mark's box {side:?} stands on whole device pixels at \
                             the bar {bar:?}'s foot, and the mark {mark:?} on the rule's rows, \
                             {rule:?}"
                        ));
                    }
                }
                (TextMode::Split, marks) => {
                    failures.push(format!("{at}: one mark on the rule {rule:?}: {marks:?}"));
                }
                (_, []) => {}
                (_, marks) => {
                    failures.push(format!("{at}: a bar of one text marks no side: {marks:?}"));
                }
            }
        });
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// The find bar's padding is as deep below its lines as above them in
    /// every case [`each_drawn_bar`] walks: in device pixels, from the bar's
    /// top to its first line and from its last line to its rule. Its lines
    /// were summed from the raw 28 px row, which layout rounds up to a
    /// device pixel as it sizes the bar's measured controls while it draws
    /// each line on its device pixel, halves toward zero: at 18 pt the
    /// controls' box was a device pixel taller than their lines at scales
    /// 1.25 and 1.5 (49 device pixels for 48 at 1.25), all of it below the
    /// last line.
    #[gpui::test]
    async fn the_find_bar_pads_its_lines_as_deep_below_as_above(cx: &mut TestAppContext) {
        let (_fixture, app, cx, _) =
            crate::width_walk::open(crate::width_walk::Page::CompareText, cx).await;
        let mut failures = Vec::new();
        each_drawn_bar(&app, cx, |cx, at, scale, _, bar, painted| {
            let lines = DRAWN_FORMS
                .with_borrow(|drawn| *drawn)
                .expect("the controls are laid out")
                .2;
            let first = drawn(cx, super::LINE_SELECTORS[0], at);
            let last = drawn(cx, super::LINE_SELECTORS[lines - 1], at);
            let rule = painted
                .rule
                .unwrap_or_else(|| panic!("{at}: the bar {bar:?} draws a rule"));
            // In device pixels, as layout draws them.
            let device = |length: Pixels| f32::from(length) * scale;
            let above = device(first.top()) - device(bar.top());
            let below = rule.top().as_f32() - device(last.bottom());
            if (above - below).abs() > 0.5 {
                failures.push(format!(
                    "{at}: the bar {bar:?} pads its {lines} lines {above} device pixels above, \
                     from {first:?}, and {below} below, to {last:?} above its rule {rule:?}"
                ));
            }
        });
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// The accessibility node `control`, a kit component, writes as it is
    /// drawn: its base element, of `witness`'s type, laid out inside the
    /// component, writes it, as `reflog`'s entry nodes are read.
    fn node_of<C: RenderOnce, W: RenderOnce>(
        control: C,
        witness: W,
        window: &mut Window,
        cx: &mut App,
    ) -> gpui::accesskit::Node {
        use gpui::Element as _;
        fn of_type<'a, E: gpui::Element>(
            _witness: &E,
            element: &'a mut AnyElement,
        ) -> Option<&'a mut E> {
            element.downcast_mut()
        }
        let mut outer = RenderOnce::render(control, window, cx).into_element();
        let (_, mut layout) = outer.request_layout(None, None, window, cx);
        let mut base = (&mut layout as &mut dyn std::any::Any)
            .downcast_mut::<Option<AnyElement>>()
            .and_then(Option::take)
            .expect("the component renders its base element");
        let witness = RenderOnce::render(witness, window, cx).into_element();
        let element = of_type(&witness, &mut base).expect("the base element writes the node");
        let mut node = gpui::accesskit::Node::new(element.a11y_role().expect("a role"));
        element.write_a11y_info(&mut node);
        node
    }

    /// Match case keeps focus, its name, its role and its checked state as a
    /// resize changes its form from the checkbox to the "Aa" toggle and
    /// back: both forms are a checked check box named Match case. The toggle
    /// was a toggle button, which AT-SPI reports pressed where it reports
    /// the checkbox checked.
    #[gpui::test]
    async fn match_case_keeps_its_focus_role_and_state_through_a_change_of_form(
        cx: &mut TestAppContext,
    ) {
        let (_fixture, app, cx, _) =
            crate::width_walk::open(crate::width_walk::Page::CompareText, cx).await;
        open_find(&app, cx, TextMode::Unified, 1480.);
        let bar = cx.update(|_, cx| {
            let editor = app.read(cx).patch_editor.clone().expect("the patch editor");
            super::panel(&editor, cx).expect("the find bar")
        });
        cx.update(|_, cx| bar.update(cx, |bar, cx| bar.match_case(true, cx)));
        let focus = cx.update(|_, cx| bar.read(cx).case_focus.clone());
        cx.update(|window, cx| window.focus(&focus, cx));
        crate::width_walk::frame(cx);
        let mut forms = Vec::new();
        for width in [1480., 461., 1480.] {
            let at = format!("resized to {width} × 490");
            cx.simulate_resize(size(px(width), px(490.)));
            crate::views::tests::draw(cx);
            let (drawn, _, _) = DRAWN_FORMS
                .with_borrow(|drawn| *drawn)
                .unwrap_or_else(|| panic!("{at}: the controls are laid out"));
            assert!(
                cx.update(|window, _| focus.is_focused(window)),
                "{at}: Match case keeps focus"
            );
            let (sensitive, held) = cx.update(|_, cx| {
                let bar = bar.read(cx);
                (bar.reading(cx).0, bar.case_focus.clone())
            });
            assert!(sensitive, "{at}: Find matches case");
            let node = crate::views::tests::view_node(cx, move |window, cx| {
                if drawn.case {
                    node_of(
                        super::match_case_box(&held, sensitive),
                        gpui_kit::base::Checkbox::new("witness"),
                        window,
                        cx,
                    )
                } else {
                    node_of(
                        super::match_case_toggle(&held, sensitive),
                        gpui_kit::base::Button::new("witness"),
                        window,
                        cx,
                    )
                }
            });
            assert_eq!(
                (node.role(), node.label(), node.toggled()),
                (
                    gpui::accesskit::Role::CheckBox,
                    Some("Match case"),
                    Some(gpui::accesskit::Toggled::True)
                ),
                "{at}: Match case's {} form",
                if drawn.case { "checkbox" } else { "\"Aa\"" }
            );
            forms.push(drawn.case);
        }
        assert_eq!(forms, [true, false, true], "the walk meets both forms");
    }

    /// The bar's measure reads the font, size and scale it lays its parts
    /// out in once: each read builds GPUI's default font features, and the
    /// count's cached width read them a second time on every frame.
    #[gpui::test]
    fn the_bar_reads_its_font_once_a_measure(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let cx = cx.add_empty_window();
        cx.update(|window, _| {
            crate::views::line_key_reads::watch(".rs");
            super::Widths::measure("3/10", px(3.), window);
            super::Widths::measure("4/10", px(3.), window);
            let reads = crate::views::line_key_reads::take();
            crate::views::line_key_reads::stop();
            assert_eq!(
                reads.shared + reads.read,
                2,
                "one read a measure: {reads:?}"
            );
        });
    }
}
