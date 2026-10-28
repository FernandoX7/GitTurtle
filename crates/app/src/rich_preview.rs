//! Captured-byte document and media information, with static native PDF pages.
use crate::{appearance::CodeFont, *};
use gitturtle_preview::{MAX_INPUT_BYTES, metadata};
use gpui_kit::base::ElementExt;
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
        if bytes.starts_with(b"%PDF-") {
            match gitturtle_preview::decode_pdf(&bytes, &check) {
                Ok(document) => {
                    side.page_count = document.page_count;
                    for page in document.pages {
                        check()?;
                        side.pages.push(Page {
                            render: Some(worker::render_image(&page)?),
                            width: page.width,
                            height: page.height,
                            caption: None,
                            error: None,
                        });
                    }
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
                side.pages.pop(),
                &check,
            )?));
            side.pages.clear();
        }
        side.capture(captured, &check)?;
        Ok(side)
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

pub(super) fn prepare_mermaid(
    old: &str,
    new: &str,
    file: &gitturtle_core::FileChange,
    check: impl Fn() -> anyhow::Result<()>,
) -> anyhow::Result<Option<Comparison>> {
    // Symlinks and unresolved LFS pointers remain literal identities, including
    // those with Mermaid suffixes; only verified content is a diagram source.
    if [file.old_mode.as_str(), file.new_mode.as_str()].contains(&"120000")
        || [old, new]
            .iter()
            .any(|source| gitturtle_preview::detect_lfs_pointer(source.as_bytes()).is_some())
    {
        return Ok(None);
    }
    let old_name = file.old_path.as_deref().unwrap_or_else(|| file.path());
    let new_name = file.new_path.as_deref().unwrap_or_else(|| file.path());
    let old_document = if file.old_path.is_some() {
        gitturtle_preview::mermaid::preview(old, old_name, 1600, &check)?
    } else {
        None
    };
    let new_document = if file.new_path.is_some() {
        gitturtle_preview::mermaid::preview(new, new_name, 1600, &check)?
    } else {
        None
    };
    if old_document.is_none() && new_document.is_none() {
        return Ok(None);
    }
    let mut sides = Vec::with_capacity(2);
    for (source, name, present, document) in [
        (old, old_name, file.old_path.is_some(), old_document),
        (new, new_name, file.new_path.is_some(), new_document),
    ] {
        check()?;
        let mut side = Side::unavailable(name, present, None);
        side.page_kind = "Diagram";
        side.metadata.format = "Mermaid diagrams".into();
        if present {
            side.capture(Arc::from(source.as_bytes()), &check)?;
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
            side.metadata
                .details
                .push("No Mermaid diagrams on this side; literal Source remains available.".into());
        }
        sides.push(Arc::new(side));
    }
    Ok(Some(Comparison {
        old: sides.remove(0),
        new: sides.remove(0),
    }))
}

/// A box for a comparison that a dialog lays out, filling the space the
/// dialog gives it, and the width it was last laid out at, which
/// [`render_comparison`] fits its open controls to. The width lives in
/// `window` under `key` while the box is drawn, so a comparison shown in it
/// later starts at that width; before the box's first layout it is zero, where
/// every open control takes its icon form.
pub(crate) fn measured_box(key: &'static str, window: &mut Window, cx: &mut App) -> (Div, Pixels) {
    let measured = window.use_keyed_state(key, cx, |_, _| None::<Pixels>);
    let width = measured.read(cx).unwrap_or_default();
    let frame = div().size_full().on_prepaint(move |bounds, _, cx| {
        measured.update(cx, |width, cx| {
            if *width != Some(bounds.size.width) {
                *width = Some(bounds.size.width);
                cx.notify();
            }
        })
    });
    (frame, width)
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

/// Render only supplied content. A dialog child must not borrow its parent
/// entity while that parent is already rendering the dialog layer; `opener`
/// opens captured bytes for the comparison's open controls, which take the
/// form the comparison's `width`, as its container was last laid out, leaves
/// them room for ([`crate::system_preview::Offer::form`]).
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
        (if quick { "Source" } else { "After" }, &preview.new),
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
            .border_b_1()
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
            .child(
                div()
                    .text_size(appearance::ui_text(11.))
                    .text_color(rgb(colors.muted))
                    .child(side.metadata.format.clone()),
            );
        let mut note = None;
        if let (Some(offer), Some(bytes)) = (side.system_preview, &side.captured) {
            let target = opener.target(("system-preview", index), label, bytes, &side.name, offer);
            // Labelled where it fits beside the side's name and format,
            // otherwise its icon.
            let room = header_room(index, side_width, label, &side.metadata.format, window);
            header = header.child(target.button(offer.form(room, window), cx));
            note = target.note(cx);
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
        // Why the last click opened nothing, on a line of its own under
        // the header's controls.
        header = header.children(note);
        let mut body = div()
            .id(("rich-side-scroll", index))
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_3()
            .flex()
            .flex_col()
            .gap_3();
        if !side.present {
            body = body.child(
                div()
                    .text_color(rgb(colors.muted))
                    .child("No file on this side"),
            );
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
                        .child(error.clone()),
                );
            }
        }
        if let Some(error) = &side.error {
            body = body.child(
                div()
                    .text_color(rgb(colors.modified))
                    .text_size(appearance::ui_text(12.))
                    .child(error.clone()),
            );
        }
        for detail in &side.metadata.details {
            body = body.child(
                div()
                    .text_size(appearance::ui_text(11.))
                    .text_color(rgb(colors.muted))
                    .child(detail.clone()),
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
                copy_feedback::copy_button(mark, "Copy source", "", cx).on_click(
                    move |_, window, cx| {
                        copy_feedback::copy(
                            window,
                            cx,
                            Some(copy_mark.clone()),
                            &what,
                            copied.to_string(),
                        )
                    },
                ),
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
            header,
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
    div()
        .size_full()
        .min_w_0()
        .flex()
        .flex_col()
        .child(div().flex().min_w_0().children(headers))
        .child(div().flex_1().min_h_0().min_w_0().flex().children(bodies))
        .into_any_element()
}

fn decoded_source(excerpt: &str, cx: &App) -> Div {
    div()
        .code_font(cx)
        .text_size(appearance::code_text())
        .child(excerpt.to_owned())
}

impl GitTurtle {
    /// The comparison in Compare's preview column, `width` wide as last laid
    /// out.
    pub(super) fn render_rich_preview(
        &self,
        preview: &Comparison,
        width: Pixels,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let opener =
            crate::system_preview::Opener::new(cx.entity().downgrade(), self.preview_showing);
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
            assert!(
                side.metadata
                    .details
                    .iter()
                    .any(|detail| detail.contains("triangles"))
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
