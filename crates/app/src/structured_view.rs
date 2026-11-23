//! Bounded tabular (CSV/TSV) and structured (JSON/TOML/YAML) preview models
//! and presentation helpers. Literal Git patches and staging stay separate.

use gitturtle_core::FileChange;
use gitturtle_preview::{
    structured::{self, StructuredFormat},
    tabular::{self, TabularPreview},
};
use gpui_kit::{
    AnyElement, Div, FocusHandle, FontWeight, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, Role, ScrollHandle, SharedString, StatefulInteractiveElement, Styled, div,
    point, px, rgb,
};
use std::path::Path;

use crate::{appearance, rich_preview};

/// Maximum rows drawn per side inside the inline table grid; remaining rows
/// remain accessible in Source/Diff tabs without UI-thread work.
const MAX_RENDERED_TABLE_ROWS: usize = 200;
const MAX_RENDERED_TABLE_COLS: usize = 24;

#[derive(Debug, Clone)]
pub struct TabularComparison {
    pub old: Option<TabularPreview>,
    pub new: Option<TabularPreview>,
}

impl TabularComparison {
    pub fn prepare(old: &str, new: &str, file: &FileChange) -> Option<Self> {
        if [file.old_mode.as_str(), file.new_mode.as_str()].contains(&"120000") {
            return None;
        }
        let old_name = file.old_path.as_deref().unwrap_or_else(|| file.path());
        let new_name = file.new_path.as_deref().unwrap_or_else(|| file.path());
        let old_table = (file.old_path.is_some()
            && rich_preview::Literal::of(&file.old_mode, old).is_none())
        .then(|| tabular::parse_tabular(old, old_name))
        .flatten();
        let new_table = (file.new_path.is_some()
            && rich_preview::Literal::of(&file.new_mode, new).is_none())
        .then(|| tabular::parse_tabular(new, new_name))
        .flatten();
        if old_table.is_none() && new_table.is_none() {
            return None;
        }
        Some(Self {
            old: old_table,
            new: new_table,
        })
    }

    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.old.as_ref().map_or(0, TabularPreview::retained_bytes)
            + self.new.as_ref().map_or(0, TabularPreview::retained_bytes)
    }
}

#[derive(Debug, Clone)]
pub struct StructuredComparison {
    pub format: StructuredFormat,
    pub old: Option<Result<String, String>>,
    pub new: Option<Result<String, String>>,
}

impl StructuredComparison {
    pub fn prepare(old: &str, new: &str, file: &FileChange) -> Option<Self> {
        if [file.old_mode.as_str(), file.new_mode.as_str()].contains(&"120000") {
            return None;
        }
        let old_name = file.old_path.as_deref().unwrap_or_else(|| file.path());
        let new_name = file.new_path.as_deref().unwrap_or_else(|| file.path());
        let format = structured::structured_format(new_name)
            .or_else(|| structured::structured_format(old_name))?;
        let side = |source: &str, name: &Path, present: bool, mode: &str| {
            if !present || rich_preview::Literal::of(mode, source).is_some() {
                return None;
            }
            structured::pretty_print(source, name).map(|res| res.map(|p| p.formatted))
        };
        let old_fmt = side(old, old_name, file.old_path.is_some(), &file.old_mode);
        let new_fmt = side(new, new_name, file.new_path.is_some(), &file.new_mode);
        if old_fmt.is_none() && new_fmt.is_none() {
            return None;
        }
        Some(Self {
            format,
            old: old_fmt,
            new: new_fmt,
        })
    }

    pub fn display_text(&self) -> Result<&str, &str> {
        match (&self.new, &self.old) {
            (Some(Ok(text)), _) => Ok(text.as_str()),
            (Some(Err(err)), _) => Err(err.as_str()),
            (None, Some(Ok(text))) => Ok(text.as_str()),
            (None, Some(Err(err))) => Err(err.as_str()),
            (None, None) => Err("No structured content available."),
        }
    }

    pub fn retained_bytes(&self) -> usize {
        let side = |s: &Option<Result<String, String>>| match s {
            Some(Ok(text)) | Some(Err(text)) => text.capacity(),
            None => 0,
        };
        std::mem::size_of::<Self>() + side(&self.old) + side(&self.new)
    }
}

pub fn render_tabular(
    comparison: &TabularComparison,
    quick_source: bool,
    colors: gitturtle_theme::Palette,
    focus: &FocusHandle,
    scroll: &ScrollHandle,
    ring: Option<Div>,
) -> AnyElement {
    let active = comparison.new.as_ref().or(comparison.old.as_ref());
    let Some(table) = active else {
        return div()
            .p_4()
            .text_color(rgb(colors.muted))
            .child("No tabular rows available.")
            .into_any_element();
    };
    let cols = table.column_count.min(MAX_RENDERED_TABLE_COLS);
    let shown_rows = table.rows.len().min(MAX_RENDERED_TABLE_ROWS);
    let kind = if table.delimiter == '\t' {
        "TSV"
    } else {
        "CSV"
    };
    let side_label = if quick_source {
        "Source"
    } else if comparison.new.is_some() {
        "After"
    } else {
        "Before"
    };
    let mut summary = format!(
        "{side_label} · {kind} · {} · {}",
        crate::counted(table.total_rows, "row", "rows"),
        crate::counted(table.column_count, "column", "columns")
    );
    if table.is_truncated() || table.rows.len() > shown_rows || table.column_count > cols {
        summary.push_str(&format!(
            " (showing first {} × {}; use Diff/Source for full file)",
            crate::counted(shown_rows, "row", "rows"),
            crate::counted(cols, "column", "columns")
        ));
    }

    let header_row = div()
        .flex()
        .border_b_1()
        .border_color(rgb(colors.border))
        .bg(rgb(colors.subtle))
        .children((0..cols).map(|col| {
            let text = table.headers.get(col).map(String::as_str).unwrap_or("");
            div()
                .w(appearance::ui_size(140.))
                .flex_shrink_0()
                .px_2()
                .py_1()
                .border_r_1()
                .border_color(rgb(colors.border))
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(appearance::code_text())
                .text_color(rgb(colors.text))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(SharedString::from(text.to_owned()))
        }));

    let body_rows = table
        .rows
        .iter()
        .take(shown_rows)
        .enumerate()
        .map(|(r, row)| {
            let row_bg = if r % 2 == 1 {
                colors.subtle
            } else {
                colors.canvas
            };
            div()
                .flex()
                .border_b_1()
                .border_color(rgb(colors.border))
                .bg(rgb(row_bg))
                .children((0..cols).map(|col| {
                    let text = row.get(col).map(String::as_str).unwrap_or("");
                    div()
                        .w(appearance::ui_size(140.))
                        .flex_shrink_0()
                        .px_2()
                        .py_1()
                        .border_r_1()
                        .border_color(rgb(colors.border))
                        .text_size(appearance::code_text())
                        .text_color(rgb(colors.text))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(SharedString::from(text.to_owned()))
                }))
        });

    let key_scroll = scroll.clone();
    let click_focus = focus.clone();
    let region_label = format!("Tabular preview: {summary}");
    div()
        .id("tabular-preview")
        .role(Role::Region)
        .aria_label(region_label)
        .relative()
        .size_full()
        .flex()
        .flex_col()
        .bg(rgb(colors.canvas))
        .track_focus(focus)
        .on_mouse_down(gpui_kit::MouseButton::Left, move |_, window, cx| {
            click_focus.focus(window, cx);
        })
        .on_key_down(move |event: &KeyDownEvent, _, cx| {
            let step = px(48.);
            let page = px(280.);
            let current = key_scroll.offset();
            let next = match event.keystroke.key.as_str() {
                "down" => Some(point(current.x, current.y - step)),
                "up" => Some(point(current.x, (current.y + step).min(px(0.)))),
                "pagedown" => Some(point(current.x, current.y - page)),
                "pageup" => Some(point(current.x, (current.y + page).min(px(0.)))),
                "home" => Some(point(px(0.), px(0.))),
                "right" => Some(point(current.x - step, current.y)),
                "left" => Some(point((current.x + step).min(px(0.)), current.y)),
                _ => None,
            };
            if let Some(offset) = next {
                cx.stop_propagation();
                key_scroll.set_offset(offset);
            }
        })
        .child(
            div()
                .px_3()
                .py_2()
                .border_b_1()
                .border_color(rgb(colors.border))
                .text_size(appearance::ui_text(12.))
                .text_color(rgb(colors.muted))
                .child(summary),
        )
        .child(
            div()
                .id("tabular-preview-scroll")
                .flex_1()
                .min_h_0()
                .overflow_scroll()
                .track_scroll(scroll)
                .child(
                    div()
                        .min_w(appearance::ui_size(140.) * cols as f32)
                        .flex()
                        .flex_col()
                        .child(header_row)
                        .children(body_rows),
                ),
        )
        .children(ring)
        .into_any_element()
}
