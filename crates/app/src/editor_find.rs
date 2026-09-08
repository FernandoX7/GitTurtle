//! Accessible Find presentation over the editor's public search engine.
//! Search never replaces the source entity or its selection, and owns a separate
//! decoration collection so patch colors survive closing or updating Find.

use crate::appearance::palette;
use gpui_kit::{
    App, AppContext, Context, DefiniteLength, Entity, EntityId, FocusHandle, Focusable, Global,
    HighlightStyle, InteractiveElement, IntoElement, ParentElement, Pixels, Render, RenderOnce,
    SharedString, StyleRefinement, Styled, Subscription, WeakEntity, Window,
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
use std::{ops::Range, rc::Rc};

const RETAINED_PANELS: usize = 64;
const MAX_QUERY_BYTES: usize = 4096;
const MAX_HIGHLIGHTS: usize = 4096;

#[derive(Default)]
struct Panels(Vec<Entity<FindBar>>);
impl Global for Panels {}

struct ReservedLayer {
    editor: WeakEntity<EditorState>,
    decorations: TextDecorationCollection,
}
#[derive(Default)]
struct ReservedLayers(Vec<ReservedLayer>);
impl Global for ReservedLayers {}

/// Reserve Find's precedence before creating any patch decoration collections.
/// This allocates one empty collection, not a Find input or panel. Handles have
/// weak source ownership; the bounded cache is pruned as previews are released.
pub fn reserve_highlight_layer(editor: &Entity<EditorState>, cx: &mut App) {
    if reserved_layer(editor, cx).is_some() {
        return;
    }
    let mut layers = std::mem::take(&mut cx.default_global::<ReservedLayers>().0);
    layers.retain(|layer| layer.editor.upgrade().is_some());
    if layers.len() >= RETAINED_PANELS {
        // Keep memory bounded even if a future caller retains many more groups;
        // the late-layer fallback uses ordinary readable text colors below.
        cx.default_global::<ReservedLayers>().0 = layers;
        return;
    }
    let decorations = editor.update(cx, |source, cx| {
        source.create_decorations_collection(Vec::new(), cx)
    });
    layers.push(ReservedLayer {
        editor: editor.downgrade(),
        decorations,
    });
    cx.default_global::<ReservedLayers>().0 = layers;
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
            native: NativeEditor::new(state),
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
    highest_priority: bool,
    painted: Option<PaintedMatches>,
    _subscriptions: Vec<Subscription>,
}
struct PaintedMatches {
    ranges: Rc<Vec<Range<usize>>>,
    current: usize,
    colors: (u32, u32, u32),
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
        let highest_priority = reserved.is_some();
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
            highest_priority,
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
        let colors = (
            colors.selected,
            if self.highest_priority {
                colors.accent
            } else {
                colors.selected
            },
            if self.highest_priority {
                colors.accent_foreground
            } else {
                colors.text
            },
        );
        if self.painted.as_ref().is_some_and(|painted| {
            Rc::ptr_eq(&painted.ranges, &ranges)
                && painted.current == current
                && painted.colors == colors
        }) {
            return;
        }
        let decorations = highlight_window(ranges.len(), current)
            .map(|index| {
                let active = index == current;
                TextDecoration::new(ranges[index].clone(), match_style(active, colors))
            })
            .collect();
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
            .text_size(px(11.))
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

fn match_style(active: bool, colors: (u32, u32, u32)) -> HighlightStyle {
    HighlightStyle {
        background_color: Some(rgb(if active { colors.1 } else { colors.0 }).into()),
        color: active.then(|| rgb(colors.2).into()),
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
    fn find_priority_composes_both_match_colors_over_split_and_unified_patch_styles() {
        use crate::appearance::ThemeChoice;
        use gpui_kit::{HighlightStyle, combine_highlights, rgb};
        for theme in ThemeChoice::ALL {
            let palette = theme.palette();
            let find = super::match_style(
                true,
                (palette.selected, palette.accent, palette.accent_foreground),
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
                    // GPUI applies collections in reverse creation order using
                    // this public compositor. Reserved Find is composed last.
                    let combined =
                        combine_highlights([(0..20, patch)], [(5..10, find)]).collect::<Vec<_>>();
                    assert_eq!(combined.len(), 3);
                    assert_eq!(combined[0], (0..5, patch));
                    assert_eq!(combined[1].0, 5..10);
                    assert_eq!(
                        combined[1].1.background_color, find.background_color,
                        "{theme:?}"
                    );
                    assert_eq!(combined[1].1.color, find.color, "{theme:?}");
                    assert_eq!(combined[2], (10..20, patch));
                    // Clearing only Find leaves the complete original patch.
                    let closed = combine_highlights([(0..20, patch)], []).collect::<Vec<_>>();
                    assert_eq!(closed, vec![(0..20, patch)]);
                }
            }
        }
    }

    #[test]
    fn find_query_limit_does_not_silently_change_unicode_search() {
        let accepted = "é".repeat(MAX_QUERY_BYTES / 2);
        assert_eq!(bounded_query(&accepted), accepted);
        assert_eq!(bounded_query(&format!("{accepted}x")), "");
        assert_eq!(bounded_query("\r\nλ filename\t"), "\r\nλ filename\t");
    }
}
