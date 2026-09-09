//! Accessible Find presentation over the editor's public search engine.
//! Search never replaces the source entity or its selection, and owns a separate
//! decoration collection so patch colors survive closing or updating Find.

use crate::appearance::palette;
use gpui_kit::{
    App, AppContext, Context, DefiniteLength, Entity, EntityId, FocusHandle, Focusable, Global,
    HighlightStyle, InteractiveElement, IntoElement, ParentElement, Pixels, Render, RenderOnce,
    SharedString, StyleRefinement, Styled, Subscription, UnderlineStyle, WeakEntity, Window,
    base::ElementExt,
    component::{
        Disableable, Sizable,
        button::{Button, ButtonVariants},
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
    patch.set(decorations, cx);
    patch
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

/// Resolved Find bounds only: excludes the editor's own padding and gutter.
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
}

impl Editor {
    pub fn new(state: &Entity<EditorState>) -> Self {
        Self {
            state: state.clone(),
            native: NativeEditor::new(state).text_size(crate::appearance::code_text()),
            height: relative(1.),
            label: "File text".into(),
        }
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
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let owner = window.current_view();
        let existing = panel(&self.state, cx);
        if let Some(panel) = &existing {
            panel.update(cx, |bar, _| {
                bar.owner = owner;
                bar.label = self.label.clone();
            });
        }
        let state = self.state.downgrade();
        let replacement = state.clone();
        let replacement_label = self.label.clone();
        let closing = existing.as_ref().map(Entity::downgrade);
        div()
            .w_full()
            .h(self.height)
            .min_h_0()
            .flex()
            .flex_col()
            .capture_action(move |_: &Search, window, cx| {
                if let Some(editor) = state.upgrade() {
                    show(&editor, owner, self.label.clone(), window, cx);
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
            .children(existing.filter(|bar| bar.read(cx).open))
            .child(div().flex_1().min_h_0().child(self.native.h(relative(1.))))
    }
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
        bar.query.update(cx, |query, cx| {
            query.focus(window, cx);
            query.set_selected_range(0..query.value().len(), cx);
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
    colors: (u32, u32),
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
        let colors = palette(cx);
        let colors = (colors.selected, colors.accent);
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

impl Render for FindBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_highlights(cx);
        let (case_sensitive, count, empty) = self
            .editor
            .upgrade()
            .map(|editor| {
                let session = editor.read(cx).search_session();
                (
                    !session.case_insensitive,
                    session.matcher.label(),
                    session.matcher.is_empty(),
                )
            })
            .unwrap_or((false, "0/0".into(), true));
        let colors = palette(cx);
        let panel = cx.weak_entity();
        let enter = cx.weak_entity();
        let escape = cx.weak_entity();
        let too_long = self.query.read(cx).value().len() > MAX_QUERY_BYTES;
        div()
            .id("accessible-editor-find")
            .track_focus(&self.focus)
            .w_full()
            .flex_none()
            .relative()
            .flex()
            .flex_col()
            .gap_1()
            .px_2()
            .py_1()
            .bg(rgb(colors.panel))
            .border_b_1()
            .border_color(rgb(colors.border))
            .text_color(rgb(colors.text))
            .text_size(crate::appearance::ui_text(11.))
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
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&self.query)
                                .small()
                                .aria_label(format!("Find in {}", self.label)),
                        ),
                    )
                    .child(
                        Button::new("close-file-find")
                            .ghost()
                            .xsmall()
                            .label("Close")
                            .accessibility_label("Close Find and return to file text")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.hide(true, window, cx)),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        Checkbox::new("file-find-match-case")
                            .label("Match case")
                            .checked(case_sensitive)
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                if let Some(editor) = this.editor.upgrade() {
                                    editor.update(cx, |source, cx| {
                                        source.set_search_query(
                                            source.search_session().query.clone(),
                                            !*checked,
                                            cx,
                                        )
                                    });
                                }
                                this.sync_highlights(cx);
                                cx.notify();
                            })),
                    )
                    .child(div().flex_1())
                    .child(div().text_color(rgb(colors.muted)).child(count))
                    .child(
                        Button::new("previous-file-match")
                            .ghost()
                            .xsmall()
                            .label("Previous")
                            .accessibility_label("Previous Find match, Shift Enter")
                            .disabled(empty)
                            .on_click(cx.listener(|this, _, _, cx| this.navigate(true, cx))),
                    )
                    .child(
                        Button::new("next-file-match")
                            .ghost()
                            .xsmall()
                            .label("Next")
                            .accessibility_label("Next Find match, Enter")
                            .disabled(empty)
                            .on_click(cx.listener(|this, _, _, cx| this.navigate(false, cx))),
                    ),
            )
            .children(too_long.then(|| {
                div()
                    .text_color(rgb(colors.warning))
                    .child("Find is limited to 4 KiB; shorten this query.")
            }))
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

fn match_style(active: bool, background_available: bool, colors: (u32, u32)) -> HighlightStyle {
    HighlightStyle {
        background_color: background_available.then(|| rgb(colors.0).into()),
        // Syntax styles supply foreground/font properties only. Avoid competing
        // with them in the pinned renderer's unordered property compositor.
        underline: (active || !background_available).then(|| UnderlineStyle {
            color: Some(rgb(colors.1).into()),
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
        for theme in ThemeChoice::ALL {
            let palette = theme.palette();
            let find = super::match_style(true, true, (palette.selected, palette.accent));
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
                            let active = styles.iter().find(|(r, _)| r.contains(&start)).unwrap().1;
                            assert_eq!(
                                active.background_color, find.background_color,
                                "{theme:?}, {visible_runs}"
                            );
                            assert_eq!(
                                active.underline, find.underline,
                                "{theme:?}, {visible_runs}"
                            );
                            assert!(
                                active.color == Some(rgb(palette.text).into())
                                    || active.color == patch.color
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
        let fallback = super::match_style(true, false, (0, 0xabcdef));
        assert!(fallback.background_color.is_none());
        assert!(fallback.color.is_none());
        assert!(fallback.underline.is_some());
    }

    #[test]
    fn find_query_limit_does_not_silently_change_unicode_search() {
        let accepted = "é".repeat(MAX_QUERY_BYTES / 2);
        assert_eq!(bounded_query(&accepted), accepted);
        assert_eq!(bounded_query(&format!("{accepted}x")), "");
        assert_eq!(bounded_query("\r\nλ filename\t"), "\r\nλ filename\t");
    }
}
