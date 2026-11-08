//! Captured-byte document and media information, with a PDF's first page
//! prepared through `pdf_view::prepare_page`.
use crate::{appearance::CodeFont, *};
use gitturtle_preview::{MAX_INPUT_BYTES, metadata};
use std::{
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
};

/// At most four external viewers or launchers at once: a Quick Look process
/// until it closes on macOS, and on Linux a portal request until it answers or
/// an `xdg-open`, which runs the application in the foreground, until it ends.
#[cfg(any(target_os = "macos", target_os = "linux"))]
static EXTERNAL_PREVIEWS: AtomicUsize = AtomicUsize::new(0);
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) struct ExternalPreviewSlot;
#[cfg(any(target_os = "macos", target_os = "linux"))]
impl ExternalPreviewSlot {
    pub(crate) fn reserve() -> anyhow::Result<Self> {
        EXTERNAL_PREVIEWS
            .try_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < 4).then_some(count + 1)
            })
            .map_err(|_| {
                anyhow::anyhow!(if cfg!(target_os = "macos") {
                    "Close an existing system preview before opening another (four-viewer limit)."
                } else {
                    "Close a file GitTurtle opened before opening another (four at a time)."
                })
            })?;
        Ok(Self)
    }
}
#[cfg(any(target_os = "macos", target_os = "linux"))]
impl Drop for ExternalPreviewSlot {
    fn drop(&mut self) {
        EXTERNAL_PREVIEWS.fetch_sub(1, Ordering::AcqRel);
    }
}

pub struct Page {
    pub render: Option<Arc<RenderImage>>,
    pub width: u32,
    pub height: u32,
    pub caption: Option<String>,
    pub error: Option<String>,
}

pub struct Side {
    pub pdf: Option<Arc<crate::pdf_view::Document>>,
    pub model: Option<Arc<crate::model_view::Document>>,
    pub metadata: metadata::Metadata,
    pub captured: Option<Arc<[u8]>>,
    /// Whether the captured bytes offer the open action, decided with them.
    pub system_preview: Option<crate::system_preview::Offer>,
    pub name: PathBuf,
    pub pages: Vec<Page>,
    pub page_count: usize,
    pub page_kind: &'static str,
    pub error: Option<String>,
    pub present: bool,
    selected_page: AtomicUsize,
}

impl Side {
    pub fn unavailable(name: &Path, present: bool, error: Option<String>) -> Self {
        Self {
            pdf: None,
            model: None,
            metadata: metadata::Metadata {
                format: if present {
                    "Preview unavailable"
                } else {
                    "Absent"
                }
                .into(),
                details: Vec::new(),
                source: None,
            },
            captured: None,
            system_preview: None,
            name: name.into(),
            pages: Vec::new(),
            page_count: 0,
            page_kind: "Page",
            error,
            present,
            selected_page: AtomicUsize::new(0),
        }
    }
    /// Retains the captured bytes and decides, here on the worker, whether
    /// they offer the open action, so a render only reads the answer.
    pub fn capture(
        &mut self,
        bytes: Arc<[u8]>,
        check: &impl Fn() -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        self.system_preview = crate::system_preview::offer(&bytes, check)?;
        self.captured = Some(bytes);
        Ok(())
    }
    pub fn prepare(
        bytes: Vec<u8>,
        name: &Path,
        check: impl Fn() -> anyhow::Result<()>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            bytes.len() <= MAX_INPUT_BYTES,
            "Captured preview exceeds the 32 MiB input limit"
        );
        check()?;
        let mut side = Self {
            pdf: None,
            model: None,
            metadata: metadata::inspect(&bytes, name),
            captured: None,
            system_preview: None,
            name: name.into(),
            pages: Vec::new(),
            page_count: 0,
            page_kind: "Page",
            error: None,
            present: true,
            selected_page: AtomicUsize::new(0),
        };
        if side.metadata.source.is_none()
            && bytes.len() <= gitturtle_core::MAX_DIFF_BYTES
            && metadata::is_literal_text(&bytes)
        {
            side.metadata.source = std::str::from_utf8(&bytes).ok().map(Arc::from);
        }
        // A PDF's first page and its text, prepared here on the repository
        // worker as the page view prepares every later page.
        let mut first_page = None;
        if bytes.starts_with(b"%PDF-") {
            match crate::pdf_view::prepare_page(&bytes, 0, &check) {
                Ok(prepared) => {
                    side.page_count = prepared.count;
                    first_page = Some(prepared.page);
                }
                Err(error) => side.error = Some(format!("{error:#}")),
            }
        } else if gitturtle_preview::model3d::is_model_path(name) {
            side.page_kind = "View";
            side.metadata.format = "3D model".into();
            match gitturtle_preview::model3d::decode_geometry(
                &bytes,
                &name.to_string_lossy(),
                &check,
            ) {
                Ok(model) => {
                    side.metadata.format = model.format;
                    // Successful supplied-byte geometry parsing supersedes the
                    // generic sniffer's filename-only uncertainty. Keep byte
                    // counts, container details and actual model restrictions.
                    side.metadata.details.retain(|detail| {
                        !(detail.starts_with("Filename hint: .")
                            && detail.ends_with("; content signature is unrecognized or corrupt."))
                    });
                    side.metadata.details.extend(model.details);
                    side.model = Some(Arc::new(crate::model_view::Document::new(model.scene)));
                }
                Err(error) => side.error = Some(format!("{error:#}")),
            }
        }
        check()?;
        let captured: Arc<[u8]> = bytes.into();
        if captured.starts_with(b"%PDF-") && side.page_count > 0 {
            side.pdf = Some(Arc::new(crate::pdf_view::Document::new(
                captured.clone(),
                side.page_count,
                first_page,
                &check,
            )?));
        }
        side.capture(captured, &check)?;
        Ok(side)
    }
    /// Whether the side holds a PDF, decided from its captured bytes as its
    /// pages are, whether or not they could be prepared: a locked PDF holds
    /// one, an absent side none.
    pub fn holds_pdf(&self) -> bool {
        self.pdf.is_some()
            || self
                .captured
                .as_deref()
                .is_some_and(|bytes| bytes.starts_with(b"%PDF-"))
    }
    pub fn retained_bytes(&self) -> usize {
        self.captured.as_ref().map_or(0, |b| b.len())
            + self.pdf.as_ref().map_or(0, |pdf| pdf.retained_bytes())
            + self
                .model
                .as_ref()
                .map_or(0, |model| model.retained_bytes())
            + self.metadata.format.capacity()
            + self
                .metadata
                .details
                .iter()
                .map(String::capacity)
                .sum::<usize>()
            + self.metadata.source.as_ref().map_or(0, |s| s.len())
            + self
                .pages
                .iter()
                .map(|p| {
                    p.render
                        .as_ref()
                        .and_then(|r| r.as_bytes(0))
                        .map_or(0, <[u8]>::len)
                        + p.caption.as_ref().map_or(0, String::capacity)
                        + p.error.as_ref().map_or(0, String::capacity)
                })
                .sum::<usize>()
            + self.name.as_os_str().len()
            + self.error.as_ref().map_or(0, String::capacity)
    }
    pub fn same_source(&self, other: &Self) -> bool {
        self.present == other.present
            && self.captured == other.captured
            && self.name == other.name
            && self.error == other.error
    }
}

pub struct Comparison {
    pub old: Arc<Side>,
    pub new: Arc<Side>,
}
impl Comparison {
    pub fn retained_bytes(&self) -> usize {
        self.old.retained_bytes() + self.new.retained_bytes()
    }
}

/// A side kept as literal text rather than read as a source of diagrams: a
/// link, whose text names a target no preview follows, or a Git LFS pointer
/// whose object the local store does not hold, whose text is only the
/// pointer. The one judgement of a side: [`prepare_mermaid`] keeps such a
/// side literal, and the rewritten-series review says why a file with no
/// side left to draw shows no diagrams.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Literal {
    Link,
    LfsPointer,
}

impl Literal {
    /// What the side stored with `mode` and holding `text` is, if it stays
    /// literal.
    pub(super) fn of(mode: &str, text: &str) -> Option<Self> {
        if mode == "120000" {
            Some(Self::Link)
        } else if gitturtle_preview::detect_lfs_pointer(text.as_bytes()).is_some() {
            Some(Self::LfsPointer)
        } else {
            None
        }
    }
    /// The side's caption beside the other side's diagrams.
    fn format(self) -> &'static str {
        match self {
            Self::Link => "Symbolic link",
            Self::LfsPointer => "Git LFS pointer",
        }
    }
    /// What the side says in the place of diagrams.
    fn reason(self) -> &'static str {
        match self {
            Self::Link => {
                "This side is stored as a link, which Diagrams never follows; literal Source \
                 remains available."
            }
            Self::LfsPointer => {
                "This side's Git LFS object is not in the local store, and no download was \
                 attempted; literal Source remains available."
            }
        }
    }
}

pub(super) fn prepare_mermaid(
    old: &str,
    new: &str,
    file: &gitturtle_core::FileChange,
    check: impl Fn() -> anyhow::Result<()>,
) -> anyhow::Result<Option<Comparison>> {
    // Symlinks and unresolved LFS pointers remain literal identities, including
    // those with Mermaid suffixes; only verified content is a diagram source.
    // Each side is judged alone, so the other side, regular text, still draws.
    let old_literal = Literal::of(&file.old_mode, old);
    let new_literal = Literal::of(&file.new_mode, new);
    let old_name = file.old_path.as_deref().unwrap_or_else(|| file.path());
    let new_name = file.new_path.as_deref().unwrap_or_else(|| file.path());
    let old_document = if file.old_path.is_some() && old_literal.is_none() {
        gitturtle_preview::mermaid::preview(old, old_name, 1600, &check)?
    } else {
        None
    };
    let new_document = if file.new_path.is_some() && new_literal.is_none() {
        gitturtle_preview::mermaid::preview(new, new_name, 1600, &check)?
    } else {
        None
    };
    if old_document.is_none() && new_document.is_none() {
        return Ok(None);
    }
    let mut sides = Vec::with_capacity(2);
    for (source, name, present, document, literal) in [
        (
            old,
            old_name,
            file.old_path.is_some(),
            old_document,
            old_literal,
        ),
        (
            new,
            new_name,
            file.new_path.is_some(),
            new_document,
            new_literal,
        ),
    ] {
        check()?;
        let mut side = Side::unavailable(name, present, None);
        side.page_kind = "Diagram";
        side.metadata.format = literal.map_or("Mermaid diagrams", Literal::format).into();
        if present {
            // A literal side's text is not captured either: capturing reads
            // it to decide what another application could open it as.
            if literal.is_none() {
                side.capture(Arc::from(source.as_bytes()), &check)?;
            }
            side.metadata.source = Some(Arc::from(source));
        }
        if let Some(document) = document {
            side.page_count = document.total;
            for (index, diagram) in document.diagrams.into_iter().enumerate() {
                check()?;
                let (render, width, height) = if let Some(image) = diagram.image {
                    (
                        Some(worker::render_image(&image)?),
                        image.width,
                        image.height,
                    )
                } else {
                    (None, 1, 1)
                };
                side.pages.push(Page {
                    render,
                    width,
                    height,
                    caption: Some(format!(
                        "Diagram {} · source line {}",
                        index + 1,
                        diagram.source_line
                    )),
                    error: diagram.error,
                });
            }
            side.metadata.details.push("Static Mermaid: flowchart, sequence, class, state, ER and pie. Source tabs retain exact text and staging.".into());
            if side.pages.len() < side.page_count {
                side.metadata.details.push(format!(
                    "First {} of {} Mermaid blocks rendered; inspect Source for all blocks.",
                    side.pages.len(),
                    side.page_count
                ));
            }
        } else if present {
            side.metadata.details.push(
                literal
                    .map_or(
                        "No Mermaid diagrams on this side; literal Source remains available.",
                        Literal::reason,
                    )
                    .into(),
            );
        }
        sides.push(Arc::new(side));
    }
    Ok(Some(Comparison {
        old: sides.remove(0),
        new: sides.remove(0),
    }))
}

thread_local! {
    /// Each side header's heading and format, laid out.
    static BEFORE_HEADING: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static AFTER_HEADING: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
}

/// The room the first line of side `index`'s header leaves its open control
/// in a side `side` wide: the side less its right edge, the header's `px_3`
/// either side, and its 12 pt semibold `heading` and 11 pt `format`, each
/// followed by `gap_2`.
fn header_room(index: usize, side: Pixels, heading: &str, format: &str, window: &Window) -> Pixels {
    let cache = if index == 0 {
        &BEFORE_HEADING
    } else {
        &AFTER_HEADING
    };
    let texts =
        crate::views::cached_text_width(cache, &format!("{heading}\n{format}"), window, || {
            crate::views::shaped_width(
                heading,
                appearance::ui_text(12.),
                FontWeight::SEMIBOLD,
                window,
            )
            .ceil()
                + crate::views::line_widths(&[(format, appearance::ui_text(11.))], window)
        });
    side - px(1.) - window.rem_size() * (0.75 * 2. + 0.5 * 2.) - texts
}

/// What a side holds, as the caption after its heading that names it:
/// its format, in 11 pt muted text, found by tests as `selector`, `-`, the
/// side's `index`, `/` and the format, a selector built only when a test
/// build sets it (`debug_selector` is a no-op in release), so drawing the
/// caption builds no string for it. Every side of this surface is headed
/// so, and in the page view a side that holds no PDF, an absent one above
/// all, so an absent side reads its absence alike beside any partner
/// (`pdf_view::side_heading`).
pub(super) fn format_caption(
    selector: &'static str,
    index: usize,
    format: SharedString,
    muted: u32,
) -> Div {
    let reads = format.clone();
    div()
        .debug_selector(move || format!("{selector}-{index}/{reads}"))
        .flex_shrink_0()
        .whitespace_nowrap()
        .font_weight(FontWeight::NORMAL)
        .text_size(appearance::ui_text(11.))
        .text_color(rgb(muted))
        .child(format)
}

/// The width [`format_caption`] lays `format` out at, cached in `cache`.
pub(super) fn format_caption_width(
    cache: &'static std::thread::LocalKey<crate::views::TextWidth>,
    format: &str,
    window: &Window,
) -> Pixels {
    crate::views::cached_text_width(cache, format, window, || {
        crate::views::line_widths(&[(format, appearance::ui_text(11.))], window)
    })
}

/// Render only supplied content. A dialog child must not borrow its parent
/// entity while that parent is already rendering the dialog layer; `opener`
/// opens captured bytes for the comparison's open controls, which take the
/// form the comparison's `width` leaves them room for
/// ([`crate::system_preview::Offer::form`]): Compare's column, or the box
/// Stashes or the rewrite review draws it in, in the frame it is drawn
/// ([`crate::in_frame`]).
pub(super) fn render_comparison<T: 'static>(
    preview: &Comparison,
    quick: bool,
    opener: crate::system_preview::Opener,
    width: Pixels,
    window: &Window,
    cx: &mut Context<T>,
) -> AnyElement {
    if preview.old.pdf.is_some() || preview.new.pdf.is_some() {
        return crate::pdf_view::render_comparison(preview, quick, opener, width, window, cx);
    }
    if [&preview.old, &preview.new]
        .into_iter()
        .any(|side| side.model.is_some() || gitturtle_preview::model3d::is_model_path(&side.name))
    {
        return crate::model_view::render_comparison(preview, quick, opener, width, window, cx);
    }
    let colors = palette(cx);
    // Each side takes an equal share.
    let side_width = width / if quick { 1. } else { 2. };
    let (headers, bodies): (Vec<_>, Vec<_>) = [
        ("Before", &preview.old),
        (
            if quick {
                crate::system_preview::ONE_SIDE
            } else {
                "After"
            },
            &preview.new,
        ),
    ]
    .into_iter()
    .enumerate()
    .filter(|(index, _)| !quick || *index == 1)
    .map(|(index, (label, side))| {
        let selected = side
            .selected_page
            .load(Ordering::Relaxed)
            .min(side.pages.len().saturating_sub(1));
        // The heading's line is as tall as the scaled 28 px controls, and
        // the headers share one row, which gives both the taller one's
        // height: Before and After align whether or not a side offers a
        // control or wraps it, its lines kept at the top.
        let mut header = div()
            .debug_selector(move || format!("rich-side-header-{index}"))
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .content_start()
            .flex_wrap()
            .gap_2()
            .px_3()
            .py_2()
            .border_r_1()
            .border_color(rgb(colors.border))
            .child(
                div()
                    .debug_selector(move || format!("rich-side-heading-{index}"))
                    .min_h(appearance::ui_size(28.))
                    .flex()
                    .items_center()
                    .text_size(appearance::ui_text(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(label),
            )
            .child(format_caption(
                "rich-side-format",
                index,
                side.metadata.format.clone().into(),
                colors.muted,
            ));
        let mut answer = None;
        if let (Some(offer), Some(bytes)) = (side.system_preview, &side.captured) {
            let target = opener.target(
                ("system-preview", index),
                label,
                bytes,
                &side.name,
                "file",
                offer,
            );
            // Labelled where it fits beside the side's name and format,
            // otherwise its icon.
            let room = header_room(index, side_width, label, &side.metadata.format, window);
            header = header.child(target.control(offer.form(room, window), cx));
            answer = target.answered(cx);
        }
        if !side.pages.is_empty() {
            for (name, delta, disabled) in [
                (
                    format!("Previous {}", side.page_kind.to_ascii_lowercase()),
                    -1isize,
                    selected == 0,
                ),
                (
                    format!("Next {}", side.page_kind.to_ascii_lowercase()),
                    1isize,
                    selected + 1 >= side.pages.len(),
                ),
            ] {
                let side = Arc::clone(side);
                header = header.child(
                    button(
                        (
                            if delta < 0 {
                                "pdf-previous"
                            } else {
                                "pdf-next"
                            },
                            index,
                        ),
                        "",
                        if delta < 0 {
                            "chevron-left"
                        } else {
                            "chevron-right"
                        },
                        false,
                    )
                    .accessibility_label(format!("{label}: {name}"))
                    .tooltip(name)
                    .disabled(disabled)
                    .on_click(cx.listener(move |_, _, window, cx| {
                        let next = side
                            .selected_page
                            .load(Ordering::Relaxed)
                            .saturating_add_signed(delta)
                            .min(side.pages.len().saturating_sub(1));
                        side.selected_page.store(next, Ordering::Relaxed);
                        window.refresh();
                        cx.notify();
                    })),
                );
            }
            header = header.child(div().text_size(appearance::ui_text(11.)).child(format!(
                "{} {} / {}{}",
                side.page_kind,
                selected + 1,
                side.page_count,
                if side.pages.len() < side.page_count {
                    format!(" · first {} available", side.pages.len())
                } else {
                    String::new()
                }
            )));
        }
        let mut body = div()
            .id(("rich-side-scroll", index))
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_3()
            .flex()
            .flex_col()
            .gap_3();
        // Why a side shows nothing, an absent file or the document's error,
        // is read as static text, as the page view's messages are
        // (`pdf_view`): drawn as plain text, a password-protected PDF's
        // sentence and the absent side's line reached no assistive
        // technology.
        if !side.present {
            body = body.child(div().text_color(rgb(colors.muted)).child(
                native_accessibility::text(
                    SharedString::from(format!("rich-side-absent-{index}")),
                    "No file on this side",
                ),
            ));
        }
        if let Some(page) = side.pages.get(selected) {
            if let Some(caption) = &page.caption {
                body = body.child(
                    div()
                        .text_size(appearance::ui_text(12.))
                        .child(caption.clone()),
                );
            }
            if let Some(render) = &page.render {
                if matches!(side.page_kind, "Diagram" | "View") {
                    // Explicit natural height avoids aspect-ratio layout
                    // retaining a pre-clamp height. Canvas containment uses
                    // the final frame bounds, so metadata stays below it.
                    body = body.child(
                        div()
                            .w_full()
                            .max_w(px(page.width as f32))
                            .h(px(page.height as f32))
                            .max_h(relative(0.7))
                            .flex_shrink_0()
                            .self_center()
                            .overflow_hidden()
                            .child(crate::gif_playback::static_image(render.clone())),
                    );
                } else {
                    // PDF pages remain width-readable with natural scrolling.
                    body = body.child(
                        div()
                            .w_full()
                            .aspect_ratio(page.width as f32 / page.height as f32)
                            .bg(rgb(0xffffff))
                            .child(crate::gif_playback::static_image(render.clone())),
                    );
                }
            }
            if let Some(error) = &page.error {
                body = body.child(
                    div()
                        .text_color(rgb(colors.modified))
                        .text_size(appearance::ui_text(12.))
                        .child(native_accessibility::text(
                            SharedString::from(format!("rich-page-message-{index}")),
                            error.clone(),
                        )),
                );
            }
        }
        if let Some(error) = &side.error {
            body = body.child(
                div()
                    .text_color(rgb(colors.modified))
                    .text_size(appearance::ui_text(12.))
                    .child(native_accessibility::text(
                        SharedString::from(format!("rich-side-message-{index}")),
                        error.clone(),
                    )),
            );
        }
        // What a side says of itself is read as static text too: a literal
        // side's reason is why it shows no diagrams.
        for (line, detail) in side.metadata.details.iter().enumerate() {
            body = body.child(
                div()
                    .text_size(appearance::ui_text(11.))
                    .text_color(rgb(colors.muted))
                    .child(native_accessibility::text(
                        SharedString::from(format!("rich-side-detail-{index}-{line}")),
                        detail.clone(),
                    )),
            );
        }
        if let Some(source) = &side.metadata.source {
            let copied = source.clone();
            let what = if quick {
                "source".to_owned()
            } else {
                format!("{label} source")
            };
            // The control's id names the side; the mark adds its path.
            let mark = copy_feedback::Mark::new(("copy-decoded-source", index), &side.name);
            let copy_mark = mark.clone();
            body = body.child(
                crate::system_preview::copy_source_button(mark, label, "Copy source", true, cx)
                    .on_click(move |_, window, cx| {
                        copy_feedback::copy(
                            window,
                            cx,
                            Some(copy_mark.clone()),
                            &what,
                            copied.to_string(),
                        )
                    }),
            );
            let mut end = source.len().min(16 * 1024);
            while !source.is_char_boundary(end) {
                end -= 1;
            }
            body = body.child(decoded_source(&source[..end], cx));
            if end < source.len() {
                body = body.child(div().text_color(rgb(colors.muted)).child(
                    "Excerpt limited to 16 KiB; Copy source includes the complete available text.",
                ));
            }
        }
        (
            (header, answer),
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .border_r_1()
                .border_color(rgb(colors.border))
                .child(body),
        )
    })
    .unzip();
    let (headers, answers): (Vec<_>, Vec<_>) = headers.into_iter().unzip();
    // Each header ends, beneath its controls, with the line saying why its
    // last click opened nothing, laid out with the bodies beneath, the
    // headers' padding beneath and their rules ending them there.
    let end = crate::system_preview::HeaderEnd {
        inset: window.rem_size() * 0.75,
        bottom: window.rem_size() * 0.5,
        right_rule: true,
        border: colors.border,
    };
    div()
        .size_full()
        .min_w_0()
        .flex()
        .flex_col()
        .child(div().flex().min_w_0().children(headers))
        .child(crate::system_preview::header_ends(
            "rich-sides",
            None,
            answers,
            end,
            div()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .flex()
                .children(bodies)
                .into_any_element(),
        ))
        .into_any_element()
}

fn decoded_source(excerpt: &str, cx: &App) -> Div {
    div()
        .code_font(cx)
        .text_size(appearance::code_text())
        .child(excerpt.to_owned())
}

impl GitTurtle {
    /// The comparison in Compare's preview column, `width` wide in the frame
    /// it is drawn.
    pub(super) fn render_rich_preview(
        &self,
        preview: &Comparison,
        width: Pixels,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let opener =
            crate::system_preview::Opener::new(cx.entity().downgrade(), self.preview_showing)
                .ringed_by(self.focus_ring.clone());
        render_comparison(preview, self.is_quick_source(), opener, width, window, cx)
    }

    /// Quick Look's copy of captured `bytes`, suffixed from what they are
    /// with `name` as a hint; the task ends with whether it opened.
    #[cfg(not(target_os = "linux"))]
    pub(crate) fn quick_look(
        &mut self,
        bytes: Arc<[u8]>,
        name: PathBuf,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), crate::system_preview::NotOpened>> {
        let response = self.preferences_writer.submit_read(move || {
            #[cfg(target_os = "macos")]
            let slot = ExternalPreviewSlot::reserve()?;
            let (directory, path) = captured_copy(&bytes, &name)?;
            #[cfg(target_os = "macos")]
            {
                let mut child = std::process::Command::new("/usr/bin/qlmanage")
                    .arg("-p")
                    .arg(&path)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()?;
                std::thread::spawn(move || {
                    let _ = child.wait();
                    drop(directory);
                    drop(slot);
                });
                Ok(())
            }
            #[cfg(not(target_os = "macos"))]
            {
                drop(directory);
                let _ = path;
                anyhow::bail!("System preview is not available on this platform")
            }
        });
        cx.spawn(async move |_, _| match response.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(format!("{error:#}").into()),
            Err(_) => Err("System preview stopped before reporting a result.".into()),
        })
    }
}

/// A private directory and newly created file prevent following any stored path
/// or symlink. Never reuse the worktree filename as a path or executable suffix.
/// Quick Look's copy; Linux writes its own through `system_preview`, typed by
/// content alone, and keeps these tests of the macOS copy running.
#[cfg(any(test, not(target_os = "linux")))]
fn captured_copy(bytes: &[u8], name: &Path) -> anyhow::Result<(tempfile::TempDir, PathBuf)> {
    use std::io::Write;
    anyhow::ensure!(
        bytes.len() <= MAX_INPUT_BYTES,
        "Captured file exceeds the 32 MiB external preview limit"
    );
    let extension = if bytes.starts_with(b"%PDF-") {
        "pdf".into()
    } else if metadata::iso_image_format(bytes) == Some("AVIF") {
        "avif".into()
    } else if metadata::iso_image_format(bytes).is_some() {
        "heic".into()
    } else if let Some(format) = metadata::jpeg2000_format(bytes) {
        if format.ends_with("JP2") {
            "jp2"
        } else {
            "j2k"
        }
        .into()
    } else if metadata::is_svg(bytes) {
        "txt".into()
    } else if let Ok(format) = image::guess_format(bytes) {
        format
            .extensions_str()
            .first()
            .copied()
            .unwrap_or("bin")
            .to_owned()
    } else {
        name.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("bin")
            .to_ascii_lowercase()
    };
    let extension = if matches!(
        extension.as_str(),
        "pdf"
            | "wav"
            | "mp3"
            | "m4a"
            | "aac"
            | "flac"
            | "ogg"
            | "mp4"
            | "mov"
            | "webm"
            | "mkv"
            | "avi"
            | "doc"
            | "xls"
            | "ppt"
            | "docx"
            | "xlsx"
            | "pptx"
            | "odt"
            | "ods"
            | "odp"
            | "zip"
            | "tar"
            | "gz"
            | "tgz"
            | "bz2"
            | "xz"
            | "7z"
            | "rar"
            | "ttf"
            | "otf"
            | "woff"
            | "woff2"
            | "png"
            | "jpg"
            | "jpeg"
            | "webp"
            | "gif"
            | "svg"
            | "bmp"
            | "tif"
            | "tiff"
            | "ico"
            | "heic"
            | "heif"
            | "avif"
            | "jp2"
            | "j2k"
            | "j2c"
            | "jpc"
            | "jpf"
            | "jpx"
            | "stl"
            | "obj"
            | "fbx"
            | "3mf"
            | "step"
            | "stp"
    ) {
        extension.as_str()
    } else if metadata::is_literal_text(bytes) {
        "txt"
    } else {
        "bin"
    };
    let directory = tempfile::Builder::new()
        .prefix("gitturtle-captured-")
        .tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let path = directory
        .path()
        .join(format!("Captured revision.{extension}"));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o400))?;
    }
    Ok((directory, path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    #[gpui::test]
    fn decoded_source_excerpts_use_the_code_font(cx: &mut TestAppContext) {
        let font = shaped_code_font(cx, |probe, cx| {
            decoded_source("a != b -> c", cx)
                .child(probe)
                .into_any_element()
        });
        assert_eq!(font.family, "Desktop Mono");
        assert_eq!(font.features.is_calt_enabled(), Some(false));
        assert!(font.features.tag_value_list().contains(&("liga".into(), 0)));
    }
    #[test]
    fn successful_model_decode_replaces_generic_uncertainty_and_keeps_real_details() {
        let mut binary_stl = vec![0; 80];
        binary_stl.extend_from_slice(&1u32.to_le_bytes());
        for value in [0f32, 0., 1., 0., 0., 0., 2., 0., 0., 0., 3., 4.] {
            binary_stl.extend_from_slice(&value.to_le_bytes());
        }
        binary_stl.extend_from_slice(&[0, 0]);
        for (name, bytes) in [
            ("binary.STL", binary_stl.as_slice()),
            (
                "mesh.stl",
                include_bytes!("../../preview/tests/fixtures/models/tetra.stl").as_slice(),
            ),
            (
                "mesh.obj",
                include_bytes!("../../preview/tests/fixtures/models/tetra.obj").as_slice(),
            ),
            (
                "mesh.fbx",
                include_bytes!("../../preview/tests/fixtures/models/tetra.fbx").as_slice(),
            ),
            (
                "mesh.step",
                include_bytes!("../../preview/tests/fixtures/models/tetra.step").as_slice(),
            ),
            (
                "mesh.3mf",
                include_bytes!("../../preview/tests/fixtures/models/tetra.3mf").as_slice(),
            ),
            (
                "mesh.GLB",
                include_bytes!("../../preview/tests/fixtures/models/glb/assembly-before.glb")
                    .as_slice(),
            ),
        ] {
            let side = Side::prepare(bytes.to_vec(), Path::new(name), || Ok(())).unwrap();
            assert!(side.error.is_none(), "{name}: {:?}", side.error);
            assert!(
                side.pages.is_empty(),
                "{name} prepares interactive geometry"
            );
            let model = side.model.as_ref().expect("retained interactive scene");
            assert!(model.retained_bytes() > 0);
            assert!(side.retained_bytes() >= bytes.len() + model.retained_bytes());
            assert_eq!(side.captured.as_deref(), Some(bytes));
            if name.ends_with(".obj") || name.ends_with(".step") {
                assert_eq!(side.metadata.source.as_deref().unwrap().as_bytes(), bytes);
            }
            assert!(
                side.metadata
                    .details
                    .contains(&format!("{} captured bytes", bytes.len()))
            );
            // The triangle count, agreeing with its noun: the binary STL's
            // one facet reads "1 triangle", where it read "1 triangles".
            let counted = if name == "binary.STL" {
                "1 triangle ·"
            } else {
                " triangles ·"
            };
            assert!(
                side.metadata
                    .details
                    .iter()
                    .any(|detail| detail.contains(counted)),
                "{name}: {:?}",
                side.metadata.details
            );
            assert!(
                !side
                    .metadata
                    .details
                    .iter()
                    .any(|detail| detail.contains("unrecognized or corrupt")),
                "{name}"
            );
        }
        let invalid = Side::prepare(vec![0; 8], Path::new("invalid.stl"), || Ok(())).unwrap();
        assert!(invalid.error.is_some());
        assert!(
            invalid
                .metadata
                .details
                .iter()
                .any(|detail| detail.contains("unrecognized or corrupt"))
        );
    }

    #[test]
    fn captured_jpeg2000_magic_preserves_container_and_codestream_suffixes() {
        for (bytes, extension) in [
            (
                include_bytes!("../../preview/tests/fixtures/half-red-blue.jp2").as_slice(),
                "jp2",
            ),
            (
                include_bytes!("../../preview/tests/fixtures/half-red-blue.j2k").as_slice(),
                "j2k",
            ),
        ] {
            let (_lease, path) = captured_copy(bytes, Path::new("misleading.png")).unwrap();
            assert_eq!(path.extension().unwrap(), extension);
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
    }
    #[test]
    fn external_copy_is_exact_private_and_does_not_use_stored_paths() {
        let bytes = b"<script>this must remain data</script>";
        let (directory, path) = captured_copy(bytes, Path::new("../../outside.html")).unwrap();
        assert!(path.starts_with(directory.path()));
        assert_eq!(path.extension().unwrap(), "txt");
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o400
            );
            assert_eq!(
                std::fs::metadata(directory.path())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
    }
    /// Before holds a desktop entry, which offers nothing; After holds PDF
    /// bytes under that launcher's name, which open as a PDF. The decision
    /// travels with the prepared side, so the render only reads it.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    fn open_in_default_app_shows_for_a_passive_type_and_not_a_launcher(cx: &mut TestAppContext) {
        struct Probe(Comparison);
        impl Render for Probe {
            fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                let opener = crate::system_preview::Opener::new(
                    WeakEntity::new_invalid(),
                    crate::system_preview::Showing::next(),
                );
                render_comparison(&self.0, false, opener, px(1000.), window, cx)
            }
        }
        cx.update(gpui_kit::init);
        let launcher = b"[Desktop Entry]\nType=Application\nName=Readme\nExec=sh -c id\n".to_vec();
        let refused = Side::prepare(launcher, Path::new("readme.desktop"), || Ok(())).unwrap();
        let pdf = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n1 0 obj\n<<>>\nendobj\n".to_vec();
        let allowed = Side::prepare(pdf, Path::new("launcher.desktop"), || Ok(())).unwrap();
        assert!(refused.captured.is_some() && allowed.captured.is_some());
        let comparison = Comparison {
            old: Arc::new(refused),
            new: Arc::new(allowed),
        };
        let (_, cx) = cx.add_window_view(move |_, _| Probe(comparison));
        for _ in 0..3 {
            cx.update(|window, cx| {
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
            });
            cx.run_until_parked();
        }
        assert!(cx.debug_bounds("system-preview-1").is_some());
        assert!(cx.debug_bounds("system-preview-0").is_none());
    }

    /// Only a side that is itself a link or a Git LFS pointer stays literal,
    /// and what it stores never reaches a decoder, even text that would
    /// parse as a diagram: the other side, regular text, draws its own.
    /// Where no side is left to draw, there is no comparison.
    #[test]
    fn prepare_mermaid_keeps_only_the_literal_side_literal() {
        let file = |old_mode: &str, new_mode: &str| gitturtle_core::FileChange {
            old_path: Some("a.mmd".into()),
            new_path: Some("a.mmd".into()),
            old_oid: None,
            new_oid: None,
            status: gitturtle_core::ChangeStatus::Modified,
            old_mode: old_mode.into(),
            new_mode: new_mode.into(),
        };
        let diagram = "flowchart LR\n  A --> B\n";
        let pointer = format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize 42\n",
            "a".repeat(64)
        );
        let link = "This side is stored as a link, which Diagrams never follows; literal \
                    Source remains available.";
        let lfs = "This side's Git LFS object is not in the local store, and no download was \
                   attempted; literal Source remains available.";
        let regular = "100644";
        for (name, old, new, old_mode, new_mode, says, literal_is_old) in [
            (
                "a link before",
                diagram,
                diagram,
                "120000",
                regular,
                link,
                true,
            ),
            (
                "a link after",
                diagram,
                diagram,
                regular,
                "120000",
                link,
                false,
            ),
            (
                "a pointer before",
                pointer.as_str(),
                diagram,
                regular,
                regular,
                lfs,
                true,
            ),
            (
                "a pointer after",
                diagram,
                pointer.as_str(),
                regular,
                regular,
                lfs,
                false,
            ),
        ] {
            let prepared = prepare_mermaid(old, new, &file(old_mode, new_mode), || Ok(()))
                .unwrap()
                .unwrap_or_else(|| panic!("{name}: the regular side draws its diagram"));
            let (literal, drawn, stored) = if literal_is_old {
                (&prepared.old, &prepared.new, old)
            } else {
                (&prepared.new, &prepared.old, new)
            };
            assert!(
                drawn.pages.len() == 1 && drawn.pages[0].error.is_none(),
                "{name}: the regular side draws one diagram"
            );
            assert!(
                literal.pages.is_empty() && literal.page_count == 0,
                "{name}: the literal side is never decoded"
            );
            assert!(
                literal.captured.is_none() && literal.system_preview.is_none(),
                "{name}: the literal side offers nothing to open"
            );
            assert_eq!(
                literal.metadata.details,
                [says],
                "{name}: the literal side says what it is"
            );
            assert_eq!(
                literal.metadata.source.as_deref(),
                Some(stored),
                "{name}: its literal text stays available"
            );
        }
        for (name, old, new, old_mode, new_mode) in [
            ("links on both sides", diagram, diagram, "120000", "120000"),
            (
                "pointers on both sides",
                pointer.as_str(),
                pointer.as_str(),
                regular,
                regular,
            ),
            (
                "a pointer then a link",
                pointer.as_str(),
                diagram,
                regular,
                "120000",
            ),
            (
                "a link then a blank file",
                diagram,
                " \n",
                "120000",
                regular,
            ),
        ] {
            assert!(
                prepare_mermaid(old, new, &file(old_mode, new_mode), || Ok(()))
                    .unwrap()
                    .is_none(),
                "{name}: no side is left to draw"
            );
        }
    }

    /// A blank Mermaid fence in Markdown is no block on the file route
    /// either: a `.md` file with a link or a pointer on one side has no
    /// Rendered view and reaches `prepare_mermaid`, where a blank fence once
    /// drew an error page and used one of the four slots.
    #[test]
    fn prepare_mermaid_leaves_out_a_markdown_files_blank_fence() {
        let file = gitturtle_core::FileChange {
            old_path: Some("a.md".into()),
            new_path: Some("a.md".into()),
            old_oid: None,
            new_oid: None,
            status: gitturtle_core::ChangeStatus::Modified,
            old_mode: "120000".into(),
            new_mode: "100644".into(),
        };
        let fence = |source: &str| format!("```mermaid\n{source}```\n\n");
        let new = format!(
            "{}{}",
            fence("\n"),
            fence("flowchart LR\n  A --> B\n").repeat(4)
        );
        let prepared = prepare_mermaid("notes.txt", &new, &file, || Ok(()))
            .unwrap()
            .expect("the regular side draws its diagrams");
        assert!(prepared.old.pages.is_empty(), "the link side has no pages");
        assert_eq!(prepared.new.pages.len(), 4, "the four real fences draw");
        assert_eq!(prepared.new.page_count, 4, "the blank fence is not counted");
        assert!(
            prepared.new.pages.iter().all(|page| page.error.is_none()),
            "no page is an error page"
        );
    }

    #[test]
    fn absent_and_empty_are_distinct_and_source_is_counted() {
        let empty = Side::prepare(vec![], Path::new("empty.bin"), || Ok(())).unwrap();
        let absent = Side::unavailable(Path::new("empty.bin"), false, None);
        assert!(!empty.same_source(&absent));
        assert!(empty.captured.is_some());
        let bytes = vec![0; 1024];
        let side = Side::prepare(bytes, Path::new("file.bin"), || Ok(())).unwrap();
        assert!(side.retained_bytes() >= 1024);
    }
}
