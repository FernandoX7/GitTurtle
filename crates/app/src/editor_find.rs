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
            Replace, Search, TextDecoration, TextDecorationCollection,
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
    decorations: TextDecorationCollection,
    patch: Option<Weak<RefCell<PatchLayer>>>,
}
#[derive(Default)]
struct ReservedLayers(Vec<ReservedLayer>);
impl Global for ReservedLayers {}

/// Reserve a Find collection without constructing its input or panel. The
/// pinned renderer composes overlapping properties in hash order, so patch
/// backgrounds are explicitly removed underneath Find instead of relying on
/// collection order. Handles retain neither released editors nor patch data.
pub fn reserve_highlight_layer(editor: &Entity<EditorState>, cx: &mut App) {
    if reserved_layer(editor, cx).is_some() {
        return;
    }
    let mut layers = std::mem::take(&mut cx.default_global::<ReservedLayers>().0);
    layers.retain(|layer| layer.editor.upgrade().is_some());
    if layers.len() >= RETAINED_PANELS {
        // Keep memory bounded if a future caller retains more groups. Find's
        // fallback still has a visible underline without competing backgrounds.
        cx.default_global::<ReservedLayers>().0 = layers;
        return;
    }
    let decorations = editor.update(cx, |source, cx| {
        source.create_decorations_collection(Vec::new(), cx)
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
            (
                layer.collection.clone(),
                without_match_backgrounds(&layer.source, &layer.matches),
            )
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
        let visible = without_match_backgrounds(&layer.source, &layer.matches);
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

fn set_match_ranges(editor: &WeakEntity<EditorState>, ranges: Vec<Range<usize>>, cx: &mut App) {
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
            if layer.matches == ranges {
                return;
            }
            layer.matches = ranges;
            (
                layer.collection.clone(),
                without_match_backgrounds(&layer.source, &layer.matches),
            )
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
    let mut output = Vec::with_capacity(source.len().saturating_add(matches.len() * 2));
    let mut first_match = 0;
    for decoration in source {
        if decoration.style.background_color.is_none() {
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
                output.push(TextDecoration::new(cursor..start, decoration.style));
            }
            if start < end {
                let mut style = decoration.style;
                style.background_color = None;
                output.push(TextDecoration::new(start..end, style));
                cursor = end;
            }
        }
        if cursor < decoration.range.end {
            output.push(TextDecoration::new(
                cursor..decoration.range.end,
                decoration.style,
            ));
        }
    }
    output
}

fn reserved_layer(editor: &Entity<EditorState>, cx: &App) -> Option<TextDecorationCollection> {
    cx.try_global::<ReservedLayers>()?
        .0
        .iter()
        .find(|layer| layer.editor.entity_id() == editor.entity_id())
        .map(|layer| layer.decorations.clone())
}

fn panel(editor: &Entity<EditorState>, cx: &App) -> Option<Entity<FindBar>> {
    cx.try_global::<Panels>()?
        .0
        .iter()
        .find(|panel| panel.read(cx).editor.entity_id() == editor.entity_id())
        .cloned()
}

/// The open find bar's height as it last prepainted, for a view that fits
/// whole lines below it (the GitHub review's source); Split's sides line up
/// in the layout pass instead ([`Editor::aligned_with`]).
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
    /// The editor whose find bar this one's rows line up with
    /// ([`Self::aligned_with`]).
    partner: Option<Entity<EditorState>>,
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
    /// Keeps this editor's rows level with `partner`'s, as Split's sides
    /// are: Find's region takes the height of the taller of the two sides'
    /// open bars, each laid out at this side's width in the frame being
    /// drawn. The sides are equally wide, so the partner's bar takes here
    /// the lines it takes there ([`FindBar::ghost`]).
    pub fn aligned_with(mut self, partner: &Entity<EditorState>) -> Self {
        self.partner = Some(partner.clone());
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
        let closing = existing.as_ref().map(Entity::downgrade);
        div()
            .w_full()
            .h(height)
            .min_h_0()
            .flex()
            .flex_col()
            .capture_action(move |_: &Search, window, cx| {
                if let Some(editor) = state.upgrade() {
                    show(&editor, owner, label.clone(), window, cx);
                    cx.stop_propagation();
                }
            })
            .capture_action(move |_: &Replace, window, cx| {
                // These readers are immutable. The inherited Replace shortcut
                // must not open the inaccessible private native overlay either.
                if let Some(editor) = replacement.upgrade() {
                    show(&editor, owner, replacement_label.clone(), window, cx);
                    cx.stop_propagation();
                }
            })
            .capture_action(move |_: &Escape, window, cx| {
                if let Some(bar) = closing.as_ref().and_then(WeakEntity::upgrade)
                    && bar.read(cx).open
                {
                    bar.update(cx, |bar, cx| bar.hide(true, window, cx));
                    cx.stop_propagation();
                }
            })
            .children(find_region(
                existing.filter(|bar| bar.read(cx).open),
                partner
                    .and_then(|partner| panel(&partner, cx))
                    .filter(|bar| bar.read(cx).open),
                window,
                cx,
            ))
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

/// Find's region above an editor's text: its own open bar, and where a
/// partner's bar is open, as Split's other side's, that bar's invisible
/// twin in the same grid cell, so the region is as tall as the taller of
/// the two in the layout pass that draws them. Laid out a frame later, from
/// heights the frame before measured, the sides' rows stood a line apart in
/// the first frame after a resize that changed a bar's line count.
fn find_region(
    own: Option<Entity<FindBar>>,
    partner: Option<Entity<FindBar>>,
    window: &Window,
    cx: &App,
) -> Option<AnyElement> {
    let Some(partner) = partner else {
        return own.map(IntoElement::into_any_element);
    };
    let cell = || div().row_start(1).col_start(1).min_w_0();
    Some(
        div()
            .w_full()
            .flex_none()
            .grid()
            .grid_cols(1)
            .children(own.map(|bar| cell().child(bar)))
            .child(cell().child(partner.read(cx).ghost(window, cx)))
            .into_any_element(),
    )
}

fn show(
    editor: &Entity<EditorState>,
    owner: EntityId,
    label: SharedString,
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
        let bar = cx.new(|cx| FindBar::new(editor, owner, label.clone(), window, cx));
        panels.push(bar.clone());
        cx.default_global::<Panels>().0 = panels;
        bar
    };
    bar.update(cx, |bar, cx| {
        bar.open = true;
        bar.owner = owner;
        bar.label = label;
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
        let query = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Find in file")
                .default_value(initial)
        });
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
        ];
        Self {
            editor: editor.downgrade(),
            owner,
            label,
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
    fn hide(&mut self, focus_source: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        self.height = px(0.);
        self.painted = None;
        self.decorations.clear(cx);
        set_match_ranges(&self.editor, Vec::new(), cx);
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

    /// The bar's box, which its twin ([`Self::ghost`]) shares, so the two
    /// take one height at one width. Spanning the line numbers, it stands
    /// its controls at a pane's content edge (`appearance::PANE_INSET`),
    /// where Split's side headings start their text.
    fn frame() -> gpui_kit::Div {
        div()
            .w_full()
            .flex_none()
            .relative()
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
    fn fit(count: SharedString, window: &Window) -> Fit {
        Fit {
            count,
            gap: window.rem_size() * 0.25,
            row: crate::appearance::ui_size(28.),
        }
    }

    /// The limit's notice below the controls.
    fn limit_notice() -> gpui_kit::Div {
        div().child("Find is limited to 4 KiB; shorten this query.")
    }

    /// This bar's invisible twin, for the other side of a Split
    /// ([`find_region`]): its box, its controls' measure and its notice,
    /// laid out as the bar is, with nothing drawn, focusable or named.
    fn ghost(&self, window: &Window, cx: &App) -> AnyElement {
        let (_, count, _) = self.reading(cx);
        Self::frame()
            .invisible()
            .child(Fitted {
                fit: Self::fit(count, window),
                controls: None,
            })
            .children(self.too_long(cx).then(Self::limit_notice))
            .into_any_element()
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
        Self::frame()
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
            // The controls take their forms from the bar's width in the frame
            // it is drawn ([`Controls`]).
            .child(Fitted {
                fit: Self::fit(count.clone(), window),
                controls: Some(Controls {
                    bar: cx.weak_entity(),
                    query: self.query.clone(),
                    label: self.label.clone(),
                    case_focus: self.case_focus.clone(),
                    case_sensitive,
                    count,
                    empty,
                    muted: colors.muted,
                }),
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
            })
    }
}

/// The text whose width is the Find field's floor: eight digits, about eight
/// characters of a query at the field's text size.
const FIELD_FLOOR: &str = "00000000";

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

thread_local! {
    /// [`Widths`] but the count, by the font, size and scale they were laid
    /// out at.
    static WIDTHS: RefCell<Option<(crate::views::LineKey, Widths)>> = const { RefCell::new(None) };
    static COUNT: crate::views::TextWidth = const { RefCell::new(None) };
}

#[cfg(test)]
thread_local! {
    /// The forms, the field's least width and the lines the find bar was
    /// last drawn with, for a test.
    pub(crate) static DRAWN_FORMS: RefCell<Option<(Forms, Pixels, usize)>> = const { RefCell::new(None) };
}

impl Widths {
    /// The parts' widths in the font the bar lays them out in, with `count`
    /// as the count and `gap` between parts. The font, size and scale are
    /// read once ([`crate::views::LineKey::current`]): each read builds
    /// GPUI's default font features, and the bar is measured every frame.
    fn measure(count: &str, gap: Pixels, window: &Window) -> Self {
        let scale = window.scale_factor();
        let device = |width: Pixels| px((f32::from(width) * scale).ceil() / scale);
        let text = |text: &str, size: Pixels| {
            crate::views::laid_out_text_width(
                crate::views::line_widths(&[(text, size)], window),
                window,
            )
        };
        let small = crate::appearance::ui_text(11.);
        let key = crate::views::LineKey::current(window);
        let count = COUNT.with_borrow_mut(|cached| match cached {
            Some((laid_out, text, width)) if *laid_out == key && text == count => *width,
            _ => {
                let width = text(count, small);
                *cached = Some((key.clone(), count.to_owned(), width));
                width
            }
        });
        let fixed = WIDTHS.with_borrow_mut(|cached| match cached {
            Some((laid_out, widths)) if *laid_out == key => *widths,
            _ => {
                let rem = window.rem_size();
                let button = |label: &str| device(crate::text_button_width_now(&[label], window));
                let widths = Self {
                    // The kit's small field: 8 px of padding and a 1 px
                    // border either side of its text, at 0.875 rem.
                    floor: device(text(FIELD_FLOOR, rem * 0.875) + px(18.)),
                    close: button("Close"),
                    previous: button("Previous"),
                    next: button("Next"),
                    // The kit's checkbox: a 1 rem box and 0.5 rem before
                    // its label at 1 rem.
                    case: device(rem * 1.5) + text("Match case", rem),
                    toggle: button("Aa"),
                    count: text(COUNT_ROOM, small),
                    icon: device(crate::appearance::ui_size(28.)),
                    gap,
                };
                *cached = Some((key, widths));
                widths
            }
        });
        Self {
            count: fixed.count.max(count),
            gap,
            ..fixed
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
        .find(|forms| self.line(*forms) <= room)
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
                if (unit[0] == Part::Case && beside_field) || used + self.gap + across > room {
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
    /// The height its lines take, `row` each, `gap` apart.
    fn height(&self, row: Pixels, gap: Pixels) -> Pixels {
        let lines = self.lines.len() as f32;
        row * lines + gap * (lines - 1.)
    }
}

/// The find bar's controls as an element: as tall as the lines they take at
/// the width layout gives them ([`Widths::plan`]), and built for that width
/// as they are prepainted, so a resize lays the bar out in the frame it is
/// drawn. Built a frame later, from the width the frame before measured, a
/// narrowing window would first draw the wider frame's forms cut. Without
/// controls, as a bar's twin ([`FindBar::ghost`]), it takes their height and
/// draws nothing.
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
    controls: Option<Controls>,
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
        let id = window.request_measured_layout(style, move |known, available, _, _| {
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
            gpui_kit::size(width, widths.plan(lines).height(row, widths.gap))
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
        let controls = self.controls.take()?;
        let (widths, planned) = measured.as_ref()?;
        // The width the measure planned from, which the drawn width holds,
        // exceeding it by less than a device pixel and a float's error; a
        // layout that measured no width it was given plans from the drawn.
        let drawn = bounds.size.width;
        let scale = window.scale_factor();
        let slack = px(1e-3);
        let width = planned
            .get()
            .filter(|planned| {
                *planned <= drawn + slack && drawn - *planned < px(1. / scale) + slack
            })
            .unwrap_or_else(|| planned_width(drawn, scale));
        let mut element = controls.rows(widths.plan(width), *widths, &self.fit);
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
    fn rows(self, plan: Plan, widths: Widths, fit: &Fit) -> AnyElement {
        use crate::system_preview::HeaderControl;
        let forms = plan.forms;
        #[cfg(test)]
        DRAWN_FORMS.with_borrow_mut(|drawn| *drawn = Some((forms, plan.field, plan.lines.len())));
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
        let field = div()
            .debug_selector(|| "file-find-field".into())
            .flex_1()
            .min_w(plan.field)
            .child(
                Input::new(&self.query)
                    .small()
                    .aria_label(format!("Find in {}", self.label)),
            );
        let count = div()
            .debug_selector(|| "file-find-count".into())
            .flex_shrink_0()
            .text_color(rgb(self.muted))
            .child(self.count);
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
            .children(plan.lines.iter().map(|parts_of_line| {
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
                let drawn = line().children(leading);
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
            super::show(&editor, editor.entity_id(), "File text".into(), window, cx);
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
    use super::{DRAWN_FORMS, FIELD_FLOOR, Forms, MAX_QUERY_BYTES, panel_height};
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
    /// 18 pt, in the unified patch and in Split's first side, whose bar,
    /// across that side's line numbers and text, is the narrowest the app
    /// draws (about 88 px at 13 pt in that window). It keeps one line, one
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
    /// Split's narrow sides at 13 and 18 pt, Before's and After's, in the
    /// windows a device pixel apart around each width where the bar's lines
    /// change, which the walk finds by bisection: the bar's last line ends as
    /// far above its 1 px rule as the first starts below its top. Layout
    /// measures the bar at its unrounded width and draws it on device pixels,
    /// rounding each edge, and the bar took its height from the first and its
    /// lines from the second: on Before's side at 13 pt in a 572.5 × 490
    /// window at the test window's scale, 2, it was three lines tall and drew
    /// four, the last below its rule, over the code. Rounding toward Before's
    /// width and away from After's, the two sides meet that at different
    /// widths, so the walk reads both.
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
            for (side, name) in [(0, "Before"), (1, "After")] {
                open_find_on(&app, cx, TextMode::Split, side, width(wide));
                // The lines drawn in a window `n` device pixels wide, whose
                // bar must be as tall as they are.
                let lines = |n: i32, cx: &mut VisualTestContext| {
                    let at = format!("{text} pt, {name}'s bar in {} × 490", width(n));
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
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        assert!(
            changes >= 8,
            "the walk meets the bar's lines changing on both sides at both sizes: {changes}"
        );
    }

    /// In Split with Find open on Before alone, the two sides' first rows
    /// stand level in every frame drawn after a resize that changes the
    /// bar's lines, the first among them, at 13 and 18 pt: After's Find
    /// region takes Before's bar's lines in the layout pass that draws it.
    /// Padded a frame later from the bar's measured height, After's rows
    /// stood a line apart from Before's in the first frame after the resize.
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

    /// In Split with Find open on both sides, Before's bar the taller for the
    /// notice under a query past Find's limit, and then on one side as each
    /// closes, the two sides' first rows stand level in every frame drawn, as
    /// with Find open on Before alone
    /// ([`split_rows_stay_level_in_the_frame_a_resize_changes_the_find_bar`]),
    /// at 13 and 18 pt: through resizes that change the bars' lines, in the
    /// frame Escape closes Before's bar, with After's alone through resizes,
    /// and in the frame it closes too. Each side's Find region takes the
    /// taller of its own open bar and the other side's.
    #[gpui::test]
    async fn split_rows_stay_level_with_find_open_on_both_sides_and_as_each_closes(
        cx: &mut TestAppContext,
    ) {
        use crate::diff_view::painted_rows;
        let (_fixture, app, cx, _) =
            crate::width_walk::open(crate::width_walk::Page::CompareText, cx).await;
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
            // Past Find's limit, Before's bar states it below its controls.
            paste(cx, &"x".repeat(MAX_QUERY_BYTES + 1));
            open_find_on(&app, cx, TextMode::Split, 1, 1480.);
            let editors = cx.update(|_, cx| {
                let split = app.read(cx).split_view.clone().expect("the split view");
                split.read(cx).editors().clone()
            });
            let sides = editors.clone().map(|editor| editor.entity_id());
            let heights = |cx: &mut VisualTestContext| {
                cx.update(|_, cx| editors.clone().map(|editor| panel_height(&editor, cx)))
            };
            let [before, after] = heights(cx);
            assert!(
                before > after,
                "{text} pt: Before's bar, {before:?}, stands taller than After's, {after:?}"
            );
            painted_rows::watch();
            for width in [461., 1480., 640., 1480.] {
                cx.simulate_resize(size(px(width), px(490.)));
                crate::views::tests::draw(cx);
                level(
                    &format!("{text} pt, both open, resized to {width} × 490"),
                    sides,
                );
            }
            // Escape in Before's text closes its bar alone.
            let before_focus = cx.update(|_, cx| editors[0].focus_handle(cx));
            cx.update(|window, cx| window.focus(&before_focus, cx));
            painted_rows::take();
            cx.simulate_keystrokes("escape");
            crate::views::tests::draw(cx);
            level(&format!("{text} pt, as Before's bar closes"), sides);
            let [before, after] = heights(cx);
            assert!(
                before == px(0.) && after > px(0.),
                "{text} pt: Before's bar closed ({before:?}) and After's open ({after:?})"
            );
            for width in [461., 1480.] {
                cx.simulate_resize(size(px(width), px(490.)));
                crate::views::tests::draw(cx);
                level(
                    &format!("{text} pt, After's alone, resized to {width} × 490"),
                    sides,
                );
            }
            let after_focus = cx.update(|_, cx| editors[1].focus_handle(cx));
            cx.update(|window, cx| window.focus(&after_focus, cx));
            painted_rows::take();
            cx.simulate_keystrokes("escape");
            crate::views::tests::draw(cx);
            level(&format!("{text} pt, as After's bar closes"), sides);
            assert_eq!(heights(cx), [px(0.); 2], "{text} pt: both bars closed");
            painted_rows::stop();
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
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
