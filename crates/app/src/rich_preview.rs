//! Captured-byte document and media information, with static native PDF pages.
use crate::*;
use gitturtle_preview::{MAX_INPUT_BYTES, metadata};
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
    pub render: Arc<RenderImage>,
    pub width: u32,
    pub height: u32,
}

pub struct Side {
    pub metadata: metadata::Metadata,
    pub captured: Option<Arc<[u8]>>,
    pub name: PathBuf,
    pub pages: Vec<Page>,
    pub page_count: usize,
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
            error: None,
            present: true,
            selected_page: AtomicUsize::new(0),
        };
        if bytes.starts_with(b"%PDF-") {
            match gitturtle_preview::decode_pdf(&bytes, &check) {
                Ok(document) => {
                    side.page_count = document.page_count;
                    for page in document.pages {
                        check()?;
                        side.pages.push(Page {
                            render: worker::render_image(&page)?,
                            width: page.width,
                            height: page.height,
                        });
                    }
                    if side.pages.len() < side.page_count {
                        side.metadata.details.push(format!("Showing the first {} of {} pages; system preview opens the complete captured document.",side.pages.len(),side.page_count));
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
                .map(|p| p.render.as_bytes(0).map_or(0, <[u8]>::len))
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

impl GitTurtle {
    pub(super) fn render_rich_preview(
        &self,
        preview: &Comparison,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = palette(cx);
        let quick = self.is_quick_source();
        div().size_full().min_w_0().flex().children([("Before",&preview.old),(if quick {"Source"}else{"After"},&preview.new)].into_iter().enumerate().filter(|(index,_)|!quick||*index==1).map(|(index,(label,side))| {
            let selected=side.selected_page.load(Ordering::Relaxed).min(side.pages.len().saturating_sub(1));
            let mut header=div().flex().items_center().flex_wrap().gap_2().px_3().py_2().border_b_1().border_color(rgb(colors.border)).child(div().text_size(appearance::ui_text(12.)).font_weight(FontWeight::SEMIBOLD).child(label)).child(div().text_size(appearance::ui_text(11.)).text_color(rgb(colors.muted)).child(side.metadata.format.clone()));
            if side.captured.is_some() {
                let captured=Arc::clone(side);
                header=header.child(button(("system-preview",index),"System preview","external-link",false).tooltip("Inspect an isolated copy of these captured revision bytes").on_click(cx.listener(move |this,_,window,cx|this.open_captured_preview(Arc::clone(&captured),window,cx))));
            }
            if !side.pages.is_empty() {
                for (name,delta,disabled) in [("Previous page",-1isize,selected==0),("Next page",1isize,selected+1>=side.pages.len())] {
                    let side=Arc::clone(side);
                    header=header.child(button((if delta<0 {"pdf-previous"}else{"pdf-next"},index),"",if delta<0 {"chevron-left"}else{"chevron-right"},false).accessibility_label(format!("{label}: {name}")).tooltip(name).disabled(disabled).on_click(cx.listener(move|_,_,_,cx| { let next=side.selected_page.load(Ordering::Relaxed).saturating_add_signed(delta).min(side.pages.len().saturating_sub(1)); side.selected_page.store(next,Ordering::Relaxed); cx.notify(); })));
                }
                header=header.child(div().text_size(appearance::ui_text(11.)).child(format!("Page {} / {}{}",selected+1,side.page_count,if side.pages.len()<side.page_count {" · first 8 available"}else{""})));
            }
            let mut body=div().id(("rich-side-scroll",index)).flex_1().min_h_0().overflow_y_scroll().p_3().flex().flex_col().gap_3();
            if !side.present { body=body.child(div().text_color(rgb(colors.muted)).child("No file on this side")); }
            if let Some(page)=side.pages.get(selected) { body=body.child(div().w_full().aspect_ratio(page.width as f32/page.height as f32).bg(rgb(0xffffff)).child(img(page.render.clone()).size_full().object_fit(ObjectFit::Contain))); }
            if let Some(error)=&side.error { body=body.child(div().text_color(rgb(colors.modified)).text_size(appearance::ui_text(12.)).child(error.clone())); }
            for detail in &side.metadata.details { body=body.child(div().text_size(appearance::ui_text(11.)).text_color(rgb(colors.muted)).child(detail.clone())); }
            if let Some(source)=&side.metadata.source {
                let copied=source.clone();
                body=body.child(button(("copy-decoded-source",index),"Copy source","copy",false).on_click(move|_,_,cx|cx.write_to_clipboard(ClipboardItem::new_string(copied.to_string()))));
                let mut end=source.len().min(16*1024); while !source.is_char_boundary(end) { end-=1; }
                body=body.child(div().font_family(mono()).text_size(appearance::code_text()).child(source[..end].to_owned()));
                if end<source.len() { body=body.child(div().text_color(rgb(colors.muted)).child("Excerpt limited to 16 KiB; Copy decoded source includes the complete decoded text.")); }
            }
            div().flex_1().min_w_0().h_full().flex().flex_col().border_r_1().border_color(rgb(colors.border)).child(header).child(body)
        })).into_any_element()
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
