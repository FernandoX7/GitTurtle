//! Captured-byte document and media information, with static native PDF pages.
use crate::*;
use gitturtle_preview::{MAX_INPUT_BYTES, metadata};
use gpui_kit::prelude::FluentBuilder;
use std::{
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
};

#[cfg(target_os = "macos")]
static EXTERNAL_PREVIEWS: AtomicUsize = AtomicUsize::new(0);
#[cfg(target_os = "macos")]
struct ExternalPreviewSlot;
#[cfg(target_os = "macos")]
impl ExternalPreviewSlot {
    fn reserve() -> anyhow::Result<Self> {
        EXTERNAL_PREVIEWS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < 4).then_some(count + 1)
            })
            .map_err(|_| {
                anyhow::anyhow!(
                    "Close an existing system preview before opening another (four-viewer limit)."
                )
            })?;
        Ok(Self)
    }
}
#[cfg(target_os = "macos")]
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
    pub metadata: metadata::Metadata,
    pub captured: Option<Arc<[u8]>>,
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
            name: name.into(),
            pages: Vec::new(),
            page_count: 0,
            page_kind: "Page",
            error,
            present,
            selected_page: AtomicUsize::new(0),
        }
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
            metadata: metadata::inspect(&bytes, name),
            captured: None,
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
                    if side.pages.len() < side.page_count {
                        side.metadata.details.push(format!("Showing the first {} of {} pages; system preview opens the complete captured document.",side.pages.len(),side.page_count));
                    }
                }
                Err(error) => side.error = Some(format!("{error:#}")),
            }
        } else if gitturtle_preview::model3d::is_model_path(name) {
            side.page_kind = "View";
            side.metadata.format = "3D model".into();
            match gitturtle_preview::model3d::decode_model(&bytes, &name.to_string_lossy(), &check)
            {
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
                    side.page_count = model.views.len();
                    for view in model.views {
                        check()?;
                        side.pages.push(Page {
                            render: Some(worker::render_image(&view.image)?),
                            width: view.image.width,
                            height: view.image.height,
                            caption: Some(view.caption),
                            error: None,
                        });
                    }
                }
                Err(error) => side.error = Some(format!("{error:#}")),
            }
        }
        check()?;
        side.captured = Some(bytes.into());
        Ok(side)
    }
    pub fn retained_bytes(&self) -> usize {
        self.captured.as_ref().map_or(0, |b| b.len())
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
            side.captured = Some(Arc::from(source.as_bytes()));
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

/// Render only supplied content. A dialog child must not borrow its parent
/// entity while that parent is already rendering the dialog layer.
pub(super) fn render_comparison<T: 'static>(
    preview: &Comparison,
    quick: bool,
    owner: WeakEntity<GitTurtle>,
    cx: &mut Context<T>,
) -> AnyElement {
    let colors = palette(cx);
    div().size_full().min_w_0().flex().children([("Before",&preview.old),(if quick {"Source"}else{"After"},&preview.new)].into_iter().enumerate().filter(|(index,_)|!quick||*index==1).map(|(index,(label,side))| {
            let selected=side.selected_page.load(Ordering::Relaxed).min(side.pages.len().saturating_sub(1));
            let mut header=div().flex().items_center().flex_wrap().gap_2().px_3().py_2().border_b_1().border_color(rgb(colors.border)).child(div().text_size(appearance::ui_text(12.)).font_weight(FontWeight::SEMIBOLD).child(label)).child(div().text_size(appearance::ui_text(11.)).text_color(rgb(colors.muted)).child(side.metadata.format.clone()));
            if side.captured.is_some() {
                let captured=Arc::clone(side);
                let target=owner.clone();
                header=header.child(button(("system-preview",index),"System preview","external-link",false).tooltip("Inspect an isolated copy of these captured revision bytes").on_click(move |_,window,cx| { let _=target.update(cx,|this,cx|this.open_captured_preview(Arc::clone(&captured),window,cx)); window.refresh(); }));
            }
            if !side.pages.is_empty() {
                for (name,delta,disabled) in [(format!("Previous {}",side.page_kind.to_ascii_lowercase()),-1isize,selected==0),(format!("Next {}",side.page_kind.to_ascii_lowercase()),1isize,selected+1>=side.pages.len())] {
                    let side=Arc::clone(side);
                    header=header.child(button((if delta<0 {"pdf-previous"}else{"pdf-next"},index),"",if delta<0 {"chevron-left"}else{"chevron-right"},false).accessibility_label(format!("{label}: {name}")).tooltip(name).disabled(disabled).on_click(cx.listener(move|_,_,window,cx| { let next=side.selected_page.load(Ordering::Relaxed).saturating_add_signed(delta).min(side.pages.len().saturating_sub(1)); side.selected_page.store(next,Ordering::Relaxed); window.refresh(); cx.notify(); })));
                }
                header=header.child(div().text_size(appearance::ui_text(11.)).child(format!("{} {} / {}{}",side.page_kind,selected+1,side.page_count,if side.pages.len()<side.page_count {format!(" · first {} available",side.pages.len())}else{String::new()})));
            }
            let mut body=div().id(("rich-side-scroll",index)).flex_1().min_h_0().overflow_y_scroll().p_3().flex().flex_col().gap_3();
            if !side.present { body=body.child(div().text_color(rgb(colors.muted)).child("No file on this side")); }
            if let Some(page)=side.pages.get(selected) {
                if let Some(caption)=&page.caption { body=body.child(div().text_size(appearance::ui_text(12.)).child(caption.clone())); }
                if let Some(render)=&page.render {
                    // PDF pages stay width-readable and scroll naturally. A
                    // diagram/model instead fits the bounded side body without
                    // enlarging small decoded images to the whole pane width.
                    let fit=matches!(side.page_kind,"Diagram"|"View");
                    body=body.child(div().w_full().aspect_ratio(page.width as f32/page.height as f32).bg(rgb(0xffffff))
                        .when(fit,|frame|frame.max_w(px(page.width as f32)).max_h(relative(0.7)).flex_shrink_0().bg(rgb(colors.panel)))
                        .child(img(render.clone()).size_full().object_fit(ObjectFit::Contain)));
                }
                if let Some(error)=&page.error { body=body.child(div().text_color(rgb(colors.modified)).text_size(appearance::ui_text(12.)).child(error.clone())); }
            }
            if let Some(error)=&side.error { body=body.child(div().text_color(rgb(colors.modified)).text_size(appearance::ui_text(12.)).child(error.clone())); }
            for detail in &side.metadata.details { body=body.child(div().text_size(appearance::ui_text(11.)).text_color(rgb(colors.muted)).child(detail.clone())); }
            if let Some(source)=&side.metadata.source {
                let copied=source.clone();
                body=body.child(button(("copy-decoded-source",index),"Copy source","copy",false).on_click(move|_,_,cx|cx.write_to_clipboard(ClipboardItem::new_string(copied.to_string()))));
                let mut end=source.len().min(16*1024); while !source.is_char_boundary(end) { end-=1; }
                body=body.child(div().font_family(mono()).text_size(appearance::code_text()).child(source[..end].to_owned()));
                if end<source.len() { body=body.child(div().text_color(rgb(colors.muted)).child("Excerpt limited to 16 KiB; Copy source includes the complete available text.")); }
            }
            div().flex_1().min_w_0().h_full().flex().flex_col().border_r_1().border_color(rgb(colors.border)).child(header).child(body)
        })).into_any_element()
}

impl GitTurtle {
    pub(super) fn render_rich_preview(
        &self,
        preview: &Comparison,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        render_comparison(preview, self.is_quick_source(), cx.entity().downgrade(), cx)
    }

    fn open_captured_preview(
        &mut self,
        side: Arc<Side>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(bytes) = side.captured.clone() else {
            return;
        };
        let name = side.name.clone();
        self.open_captured_bytes(bytes, name, window, cx);
    }
    pub(super) fn open_captured_bytes(
        &mut self,
        bytes: Arc<[u8]>,
        name: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let response=self.preferences_writer.submit_read(move|| {
            #[cfg(target_os="macos")]
            let slot=ExternalPreviewSlot::reserve()?;
            let (directory,path)=captured_copy(&bytes,&name)?;
            #[cfg(target_os="macos")]
            {
                let mut child=std::process::Command::new("/usr/bin/qlmanage").arg("-p").arg(&path).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn()?;
                std::thread::spawn(move|| { let _=child.wait(); drop(directory); drop(slot); });
                Ok(())
            }
            #[cfg(not(target_os="macos"))]
            {
                drop(directory);
                let _=path;
                anyhow::bail!("System Quick Look requires macOS; native external preview is unavailable on this platform")
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = response.await;
            let _ = this.update_in(cx, |this, _, cx| {
                this.operation_notice = Some(match result {
                    Ok(Ok(())) => {
                        "System preview opened a read-only copy of the captured file bytes.".into()
                    }
                    Ok(Err(error)) => format!("System preview could not open: {error:#}"),
                    Err(_) => "System preview stopped before reporting a result.".into(),
                });
                cx.notify();
            });
        })
        .detach();
    }
}

/// A private directory and newly created file prevent following any stored path
/// or symlink. Never reuse the worktree filename as a path or executable suffix.
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
        ] {
            let side = Side::prepare(bytes.to_vec(), Path::new(name), || Ok(())).unwrap();
            assert!(side.error.is_none(), "{name}: {:?}", side.error);
            assert_eq!(side.pages.len(), 4);
            assert_eq!(side.captured.as_deref(), Some(bytes));
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
