//! On-demand captured-byte PDF navigation. [`prepare_page`] prepares one
//! page and its text off the UI thread: on macOS with CoreGraphics and PDFKit,
//! whose references stay on the preparing thread, and on Linux (x86-64 and
//! AArch64) in GitTurtle's confined helper process (`crate::pdf_helper`).
//! The first page is prepared on the repository worker and every later one
//! on one serialized render lane; each side retains at most four pages /
//! 16 MiB.
use crate::native_accessibility::LiveRegion as _;
use crate::system_preview::HeaderControl;
use crate::*;
use gpui_kit::base::ElementExt;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::prelude::FluentBuilder as _;
use std::{
    collections::VecDeque,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

const CACHE_BYTES: usize = 16 * 1024 * 1024;
const CACHE_PAGES: usize = 4;

/// What a page says until it arrives, the one sentence for that wait: in the
/// page's room, whether or not its request has reached the render lane yet,
/// and as why Read page text is unavailable, in its tooltip and accessible
/// name. The room once said "Page preparation pending…" in the frame that
/// turned to the page and "Preparing selected page…" once a redraw found it
/// requested.
const PREPARING: &str = "Preparing the page…";

/// The link's readings, independent and linked.
const LINK_READINGS: [&str; 2] = ["Independent pages", "Linked pages"];

/// The serialized lane that prepares every page after a side's first. Each
/// test thread has a lane of its own, so a test's pages never wait behind
/// other tests' pages or meet a bounded queue they filled: one lane for the
/// whole test process, held by one test's slow helper, filled with other
/// tests' requests and refused the next as busy ("Background executor is
/// busy or unavailable"), failing tests that waited for their page.
fn renderer() -> &'static SerialExecutor {
    #[cfg(not(test))]
    {
        static RENDERER: std::sync::OnceLock<SerialExecutor> = std::sync::OnceLock::new();
        RENDERER.get_or_init(|| SerialExecutor::new("gitturtle-pdf-pages"))
    }
    #[cfg(test)]
    {
        thread_local! {
            static RENDERER: &'static SerialExecutor =
                Box::leak(Box::new(SerialExecutor::new("gitturtle-pdf-pages")));
        }
        RENDERER.with(|renderer| *renderer)
    }
}

#[derive(Default)]
struct Cache {
    pages: VecDeque<(usize, Arc<PreparedPage>, usize)>,
    bytes: usize,
}
impl Cache {
    fn get(&mut self, index: usize) -> Option<Arc<PreparedPage>> {
        let position = self.pages.iter().position(|(page, _, _)| *page == index)?;
        let item = self.pages.remove(position)?;
        let result = item.1.clone();
        self.pages.push_back(item);
        Some(result)
    }
    fn insert(&mut self, index: usize, page: impl Into<PreparedPage>) {
        let page = page.into();
        let bytes = page
            .render
            .as_ref()
            .and_then(|image| image.as_bytes(0))
            .map_or(0, <[u8]>::len)
            + page.text.as_ref().map_or(0, |text| text.len())
            + page.text_notice.len();
        if bytes > CACHE_BYTES {
            return;
        }
        if let Some(position) = self.pages.iter().position(|(page, _, _)| *page == index)
            && let Some((_, _, old)) = self.pages.remove(position)
        {
            self.bytes -= old;
        }
        while self.pages.len() >= CACHE_PAGES || self.bytes + bytes > CACHE_BYTES {
            let Some((_, _, old)) = self.pages.pop_front() else {
                break;
            };
            self.bytes -= old;
        }
        self.bytes += bytes;
        self.pages.push_back((index, Arc::new(page), bytes));
    }
}
/// A page's pixels ready to draw, its text, and what the text dialog says
/// about it.
pub(crate) struct PreparedPage {
    image: rich_preview::Page,
    text: Option<Arc<str>>,
    text_notice: String,
}
impl std::ops::Deref for PreparedPage {
    type Target = rich_preview::Page;
    fn deref(&self) -> &Self::Target {
        &self.image
    }
}
impl From<rich_preview::Page> for PreparedPage {
    fn from(image: rich_preview::Page) -> Self {
        Self {
            image,
            text: None,
            text_notice: "Page text has not been prepared.".into(),
        }
    }
}
impl PreparedPage {
    fn new(
        image: rich_preview::Page,
        text: anyhow::Result<gitturtle_preview::PdfPageText>,
    ) -> Self {
        match text {
            Ok(text)=>Self {image,text_notice:if text.truncated {"Extracted text is limited to 256 KiB. Reading order can differ from the page layout."}else if text.text.is_none() {"This page has no extractable text. Scanned images require an external OCR tool."}else{"Extracted PDF text; reading order can differ from the page layout."}.into(),text:text.text.map(Arc::from)},
            Err(error)=>Self {image,text:None,text_notice:format!("Page text is unavailable: {error:#}")},
        }
    }
}

/// A prepared page and the page count of the document it came from.
pub(crate) struct Prepared {
    pub(crate) count: usize,
    pub(crate) page: PreparedPage,
}

/// Prepares zero-based `page` of `bytes`, its pixels converted for drawing
/// and its text, for both call sites: the first page on the repository
/// worker (`rich_preview::Side::prepare`) and every later one on the render
/// lane ([`Document::request`]). Call it only from those threads, which
/// outlive the call: on Linux the helper's parent-death signal follows the
/// thread that started it. `check` runs between phases, and on Linux at
/// least every 25 ms while the helper runs, which kills it as soon as the
/// check fails. Errors are worded for a person.
pub(crate) fn prepare_page(
    bytes: &[u8],
    page: usize,
    check: &dyn Fn() -> anyhow::Result<()>,
) -> anyhow::Result<Prepared> {
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    {
        let reply = helper::client(bytes)
            .render(bytes, page, check)
            .map_err(helper::worded)?;
        check()?;
        let render = worker::render_image(&reply.image)?;
        check()?;
        let text = reply.text.map_err(|kind| {
            helper::worded(crate::pdf_helper::Failure::Document {
                kind,
                page: reply.page,
            })
        });
        Ok(Prepared {
            count: reply.page_count,
            page: PreparedPage::new(
                rich_preview::Page {
                    width: reply.image.width,
                    height: reply.image.height,
                    render: Some(render),
                    caption: None,
                    error: None,
                },
                text,
            ),
        })
    }
    // macOS renders with CoreGraphics and reads text with PDFKit; any other
    // platform refuses with the preview crate's wording.
    #[cfg(not(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )))]
    {
        let mut preview = gitturtle_preview::decode_pdf_page(bytes, page, check)?;
        check()?;
        let image = preview
            .pages
            .pop()
            .ok_or_else(|| anyhow::anyhow!("PDF page is unavailable"))?;
        let render = worker::render_image(&image)?;
        check()?;
        let text = gitturtle_preview::decode_pdf_page_text(bytes, page, check);
        check()?;
        Ok(Prepared {
            count: preview.page_count,
            page: PreparedPage::new(
                rich_preview::Page {
                    width: image.width,
                    height: image.height,
                    render: Some(render),
                    caption: None,
                    error: None,
                },
                text,
            ),
        })
    }
}

/// The Linux helper's client as the page view uses it.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod helper {
    use crate::pdf_helper::{Client, Failure};
    use gitturtle_preview::pdf_helper::ErrorKind;
    use std::sync::Arc;

    /// The client that renders `bytes`' pages: this executable as the
    /// confined helper, with the app's two helper slots. In tests, the
    /// helper a test named for these bytes, or else one that cannot start.
    pub(super) fn client(bytes: &[u8]) -> Arc<Client<'static>> {
        #[cfg(test)]
        {
            super::tests::linux::helper_for(bytes)
        }
        #[cfg(not(test))]
        {
            let _ = bytes;
            static CLIENT: std::sync::OnceLock<Arc<Client<'static>>> = std::sync::OnceLock::new();
            CLIENT.get_or_init(|| Arc::new(Client::new())).clone()
        }
    }

    /// `failure` in the words a person reads, which only `Failure`'s
    /// `Display` gives: its `Debug` form carries error kinds and signals and
    /// never reaches a page, a log or a label. A cancelled request keeps its
    /// check's own error.
    pub(super) fn worded(failure: Failure) -> anyhow::Error {
        match failure {
            Failure::Cancelled(error) => error,
            // The view asks only for pages within the count a validated first
            // reply gave it, so a reply that puts the page outside the
            // document contradicts that count. Its own count would contradict
            // the page number beside it ("Page 4 / 5"), so the view shows
            // neither and calls the reply unreadable.
            Failure::Document {
                kind: ErrorKind::PageOutOfRange { .. },
                ..
            } => anyhow::anyhow!("{}", Failure::Unreadable),
            failure => anyhow::anyhow!("{failure}"),
        }
    }
}

struct State {
    page: usize,
    zoom: f32,
    generation: u64,
    pending: bool,
    error: Option<String>,
    cache: Cache,
}
impl State {
    fn select(&mut self, page: usize, count: usize) {
        let page = page.min(count.saturating_sub(1));
        if page != self.page {
            self.page = page;
            self.generation = self.generation.wrapping_add(1);
            self.pending = false;
            self.error = None;
        }
    }
    fn accepts(&self, generation: u64, page: usize) -> bool {
        self.generation == generation && self.page == page && self.pending
    }
}

pub struct Document {
    bytes: Arc<[u8]>,
    pub count: usize,
    state: Mutex<State>,
    linked: AtomicBool,
}
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Bookmark {
    page: usize,
    zoom: f32,
    linked: bool,
}
impl Document {
    pub fn bookmark(&self) -> Bookmark {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        Bookmark {
            page: state.page,
            zoom: state.zoom,
            linked: self.linked.load(Ordering::Relaxed),
        }
    }
    pub fn restore(&self, bookmark: Bookmark) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.select(bookmark.page, self.count);
        state.zoom = if bookmark.zoom.is_finite() {
            bookmark.zoom.clamp(0.25, 4.)
        } else {
            1.
        };
        self.linked.store(bookmark.linked, Ordering::Relaxed);
    }
    /// A document of `count` pages, its first page already prepared with its
    /// text where `first` holds it ([`prepare_page`]).
    pub fn new(
        bytes: Arc<[u8]>,
        count: usize,
        first: Option<PreparedPage>,
        check: impl Fn() -> anyhow::Result<()>,
    ) -> anyhow::Result<Self> {
        check()?;
        let mut cache = Cache::default();
        if let Some(page) = first {
            cache.insert(0, page);
        }
        Ok(Self {
            bytes,
            count,
            state: Mutex::new(State {
                page: 0,
                zoom: 1.,
                generation: 0,
                pending: false,
                error: None,
                cache,
            }),
            linked: AtomicBool::new(false),
        })
    }
    pub fn retained_bytes(&self) -> usize {
        // Reserve the full cache allowance in the immutable parent cache's
        // accounting so demand-loaded pages cannot silently exceed its budget.
        CACHE_BYTES + std::mem::size_of::<Self>()
    }
    pub fn pause(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.generation = state.generation.wrapping_add(1);
        state.pending = false;
    }
    fn select(&self, page: usize, partner: Option<&Document>) {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .select(page, self.count);
        if self.linked.load(Ordering::Relaxed)
            && let Some(partner) = partner
        {
            partner
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .select(page, partner.count);
        }
    }
    fn request<T: 'static>(self: &Arc<Self>, window: &mut Window, cx: &mut Context<T>) {
        let (generation, page) = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let page = state.page;
            if state.pending || state.error.is_some() || state.cache.get(page).is_some() {
                return;
            }
            state.pending = true;
            (state.generation, page)
        };
        let document = Arc::downgrade(self);
        let bytes = self.bytes.clone();
        let response = renderer().submit_read(move || {
            // Fails once the page changes, the view pauses (Back, a hidden
            // tab or another file) or the document is dropped. On Linux the
            // helper's client runs it while the helper works and kills the
            // helper as soon as it fails.
            let check = || {
                anyhow::ensure!(
                    document.upgrade().is_some_and(|document| document
                        .state
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .accepts(generation, page)),
                    "PDF page request superseded"
                );
                Ok(())
            };
            check()?;
            Ok(prepare_page(&bytes, page, &check)?.page)
        });
        let document = Arc::downgrade(self);
        cx.spawn_in(window, async move |owner, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "PDF page preparation ended without a result"
                ))
            });
            let Some(document) = document.upgrade() else {
                return;
            };
            {
                let mut state = document.state.lock().unwrap_or_else(|e| e.into_inner());
                if !state.accepts(generation, page) {
                    return;
                }
                state.pending = false;
                match result {
                    Ok(page_image) => state.cache.insert(page, page_image),
                    Err(error) => state.error = Some(format!("{error:#}")),
                }
            }
            let _ = owner.update_in(cx, |_, window, cx| {
                window.refresh();
                cx.notify();
            });
        })
        .detach();
    }
}

pub(super) fn pause(content: Option<&Content>) {
    if let Some(Content::Rich(comparison)) = content {
        for side in [&comparison.old, &comparison.new] {
            if let Some(document) = &side.pdf {
                document.pause();
            }
        }
    }
}

fn read_page_text(
    page: Arc<PreparedPage>,
    label: &'static str,
    number: usize,
    window: &mut Window,
    cx: &mut App,
) {
    let return_focus = window.focused(cx);
    let editor = page
        .text
        .as_ref()
        .map(|text| text::editor(text, "text", None, window, cx));
    let focus = editor
        .as_ref()
        .map(|editor| editor.read(cx).focus_handle(cx));
    let height = (window.viewport_size().height * 0.6).min(px(640.));
    window.open_alert_dialog(cx, move |dialog, window, _| {
        let return_focus = return_focus.clone();
        let mut body = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(native_accessibility::text(
                "pdf-text-explanation",
                page.text_notice.clone(),
            ));
        if let Some(editor) = &editor {
            body = body.child(
                div().h(height).child(
                    editor_find::Editor::new(editor)
                        .readonly(true)
                        .aria_label(format!("{label} PDF page {number}: extracted text"))
                        .h_full(),
                ),
            );
        }
        dialog
            .title(format!("{label} · Page {number} text"))
            .width(appearance::dialog_width(px(800.), window))
            .max_h(appearance::dialog_max_height(window))
            .child(body)
            .footer(DialogFooter::new().child(
                button("close-pdf-text", "Back to PDF", "", false).on_click(
                    move |_, window, cx| {
                        window.close_dialog(cx);
                        if let Some(focus) = &return_focus {
                            focus.focus(window, cx);
                        }
                        window.refresh();
                    },
                ),
            ))
    });
    if let Some(focus) = focus {
        window.on_next_frame(move |window, cx| focus.focus(window, cx));
    }
}

fn page_picker(
    document: Arc<Document>,
    partner: Option<Arc<Document>>,
    label: &'static str,
    window: &mut Window,
    cx: &mut App,
) {
    let current = document
        .state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .page
        + 1;
    let count = document.count;
    let validation = cx.new(|_| String::new());
    let validation_result = validation.clone();
    let return_focus = window.focused(cx);
    let accepted_focus = return_focus.clone();
    let input = cx.new(|cx| {
        InputState::new(window, cx)
            .default_value(current.to_string())
            .placeholder("Page number")
    });
    let opened = input.clone();
    let accept = Arc::new(
        move |window: &mut Window, cx: &mut App, input: &Entity<InputState>| -> bool {
            if let Ok(page) = input.read(cx).value().trim().parse::<usize>()
                && (1..=document.count).contains(&page)
            {
                document.select(page - 1, partner.as_deref());
                window.close_dialog(cx);
                if let Some(focus) = &accepted_focus {
                    focus.focus(window, cx);
                }
                window.refresh();
                true
            } else {
                validation_result.update(cx, |message, cx| {
                    *message = format!("Enter a whole page number from 1 to {count}.");
                    cx.notify();
                });
                window.refresh();
                false
            }
        },
    );
    window.open_alert_dialog(cx, move |dialog, window, cx| {
        let field = input.clone();
        let enter = accept.clone();
        let click = accept.clone();
        let clicked_input = input.clone();
        let return_focus = return_focus.clone();
        dialog
            .title(format!("{label}: go to page"))
            .width(appearance::dialog_width(px(360.), window))
            .max_h(appearance::dialog_max_height(window))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(format!("Choose a page from 1 to {count}."))
                    .child(Input::new(&input).aria_label(format!("{label}: PDF page number")))
                    .child(
                        div()
                            .id("pdf-page-validation")
                            .alert(validation.read(cx).clone())
                            .child(validation.read(cx).clone()),
                    ),
            )
            .footer(
                DialogFooter::new()
                    .child(button("cancel-pdf-page", "Cancel", "", false).on_click(
                        move |_, window, cx| {
                            window.close_dialog(cx);
                            if let Some(focus) = &return_focus {
                                focus.focus(window, cx);
                            }
                            window.refresh();
                        },
                    ))
                    .child(button("open-pdf-page", "Go to page", "", true).on_click(
                        move |_, window, cx| {
                            click(window, cx, &clicked_input);
                        },
                    )),
            )
            .on_ok(move |_, window, cx| {
                enter(window, cx, &field);
                false
            })
    });
    // The field opens focused with the current page selected, as a native
    // page field does, so the number typed replaces it: with the caret
    // before it, typing 9 on page 4 read 94.
    window.on_next_frame(move |window, cx| {
        opened.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        })
    });
}

thread_local! {
    /// Each side header's heading, laid out.
    static BEFORE_HEADING: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static AFTER_HEADING: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    /// Each side's name alone, its heading where the whole does not fit.
    static BEFORE_NAME: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static AFTER_NAME: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    /// Each side's format caption, for a side that holds no PDF.
    static BEFORE_FORMAT: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static AFTER_FORMAT: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    /// The texts every paged side's header draws whatever its page
    /// ([`header_texts`]), laid out.
    static HEADER_TEXTS: std::cell::RefCell<Option<(crate::views::LineKey, HeaderTexts)>> =
        const { std::cell::RefCell::new(None) };
    /// Each side's page number, whole and short, at its widest for a count
    /// of so many digits ([`page_number_widths`]), laid out.
    static BEFORE_PAGE_NUMBER: PageNumberWidths = const { std::cell::RefCell::new(None) };
    static AFTER_PAGE_NUMBER: PageNumberWidths = const { std::cell::RefCell::new(None) };
}

/// A side's page number's texts laid out whole and short at their widest,
/// in whole pixels, by what they were laid out with and the count's digits.
type PageNumberWidths =
    std::cell::RefCell<Option<(crate::views::LineKey, usize, (Pixels, Pixels))>>;

/// The texts every paged side's header draws whatever its page, each in
/// whole pixels at its widest reading as a text element lays it out
/// (`views::line_widths`), laid out once per font and size: the display's
/// scale, which the cache's key does not hold, places each control on its
/// device pixels as it is read ([`crate::placed_button_width`]).
#[derive(Clone, Copy)]
struct HeaderTexts {
    /// Read page text and Retry page, the wider, at 12 pt.
    page: Pixels,
    /// The link's readings ([`LINK_READINGS`]), the wider, at 12 pt.
    link: Pixels,
    /// Fit, − and +, at 12 pt.
    zooms: [Pixels; 3],
    /// The zoom's widest reading, "400%", at 10 pt.
    level: Pixels,
    /// The compact zoom menu's widest reading, at 12 pt: every zoom from
    /// 25% to 400% reads at most three digits, so it is measured at three
    /// of each digit, as a page number is ([`page_number_readings`]).
    zoom_menu: Pixels,
}

/// [`HeaderTexts`], laid out once per font and size.
fn header_texts(window: &Window) -> HeaderTexts {
    let key = crate::views::LineKey::current(window);
    HEADER_TEXTS.with_borrow_mut(|cached| match cached {
        Some((laid_out, texts)) if *laid_out == key => *texts,
        _ => {
            let widest = |readings: &[&str], size: Pixels| {
                readings
                    .iter()
                    .map(|reading| crate::views::line_widths(&[(reading, size)], window))
                    .fold(Pixels::ZERO, Pixels::max)
            };
            let label = appearance::ui_text(12.);
            let menu: Vec<String> = ('0'..='9')
                .map(|digit| format!("{}%", digit.to_string().repeat(3)))
                .collect();
            let menu: Vec<&str> = menu.iter().map(String::as_str).collect();
            let texts = HeaderTexts {
                page: widest(&["Read page text", "Retry page"], label),
                link: widest(&LINK_READINGS, label),
                zooms: ["Fit", "−", "+"].map(|zoom| widest(&[zoom], label)),
                level: widest(&["400%"], appearance::ui_text(10.)),
                zoom_menu: widest(&menu, label),
            };
            *cached = Some((key, texts));
            texts
        }
    })
}

/// Read page text and Retry page as layout places them, labelled after
/// their icon, at the wider ([`crate::placed_button_width`]): both take
/// this width, so a page that fails moves nothing after it.
fn page_control_width(window: &Window) -> Pixels {
    crate::placed_button_width(header_texts(window).page, true, window)
}

/// The link labelled as layout places it, at its wider reading
/// ([`crate::placed_button_width`]), so a click moves nothing after it.
fn link_width(window: &Window) -> Pixels {
    crate::placed_button_width(header_texts(window).link, false, window)
}

/// The zoom's widest reading, "400%", as layout places it: the level keeps
/// this width, so a zoom never moves what follows it.
fn zoom_level_width(window: &Window) -> Pixels {
    crate::views::laid_out_text_width(header_texts(window).level, window)
}

#[cfg(test)]
thread_local! {
    /// How many times a page number's readings were built, for a test.
    pub(crate) static PAGE_NUMBER_READINGS_BUILT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A side's heading as the first line of its header draws it
/// ([`side_heading`]).
#[derive(Clone)]
struct Heading {
    /// What its semibold text reads.
    text: String,
    /// The side's own format, as the caption after that text, for a side
    /// that holds no PDF ([`rich_preview::format_caption`]).
    caption: Option<SharedString>,
    /// Their width together, with the gap between them.
    width: Pixels,
    /// Whether it reads its kind, "· PDF" or its format's caption, rather
    /// than the side's name alone.
    kind: bool,
}

/// Side `index`'s heading, as wide as layout draws it on this display's
/// device pixels, headed by what `side` holds and never by its partner: for
/// a side that holds a PDF, "Before · PDF" where a line of the header holds
/// it (`line`); for one that holds none, an absent side above all, its name
/// and its own format as the caption the generic document surface heads
/// every side with ("Before", "Absent"), so an absent side reads its absence
/// alike beside an added PDF, whose pages this view draws, and beside a
/// locked one, which leaves both sides to that surface. Where a line does
/// not hold it, the heading is the
/// side's name alone, "Before", the format giving way first as a caption
/// does in the one responsive order; where even the name does not fit, it
/// ends in an ellipsis (`render_comparison`). The heading was once laid out
/// whole in every side and cut at the side's edge, "Before · PD" at 18 pt in
/// a 461 × 490 window, and every side read "· PDF", so an absent Before
/// read "Before · PDF", its partner's format, beside an added PDF and
/// "Before Absent" beside a locked one. Measured in whole pixels alone, the
/// heading was drawn up to a device pixel wider than measured at scale 1.2,
/// at 15 pt 130 device pixels for 129, so a line that held the measure cut
/// the name where its caption had been chosen. Where `kind` is false it
/// reads the name alone, as its partner's heading does where its line does
/// not hold its own kind ([`pair_headings`]).
fn side_heading(
    index: usize,
    label: &str,
    side: &rich_preview::Side,
    line: Pixels,
    kind: bool,
    window: &Window,
) -> Heading {
    let [whole_cache, name_cache, format_cache] = if index == 0 {
        [&BEFORE_HEADING, &BEFORE_NAME, &BEFORE_FORMAT]
    } else {
        [&AFTER_HEADING, &AFTER_NAME, &AFTER_FORMAT]
    };
    // Each text as layout draws it on this display's device pixels: shaped
    // semibold and rounded up to a whole pixel, cached so, since the cache's
    // key does not hold the display's scale, then up to a device pixel
    // (`crate::views::laid_out_text_width`).
    let measure = |cache, text: &str| {
        let whole = crate::views::cached_text_width(cache, text, window, || {
            let size = window.text_style().font_size.to_pixels(window.rem_size());
            crate::views::shaped_width(text, size, FontWeight::SEMIBOLD, window).ceil()
        });
        crate::views::laid_out_text_width(whole, window)
    };
    let name = measure(name_cache, label);
    let (text, caption, width) = if side.holds_pdf() {
        let whole = format!("{label} · PDF");
        let width = measure(whole_cache, &whole);
        (whole, None, width)
    } else {
        let format = side.metadata.format.as_str();
        // The name, the `gap_2` on a whole device pixel, halves toward zero
        // (`Window::pixel_snap`), and the caption as drawn, summed on their
        // device pixel.
        let caption = rich_preview::format_caption_width(format_cache, format, window);
        let width = crate::image_compare::on_device_pixel(
            name + window.pixel_snap(window.rem_size() * 0.5)
                + crate::views::laid_out_text_width(caption, window),
            window,
        );
        (label.to_owned(), Some(format.to_owned().into()), width)
    };
    if kind && width <= line {
        Heading {
            text,
            caption,
            width,
            kind: true,
        }
    } else {
        Heading {
            text: label.to_owned(),
            caption: None,
            width: name.min(line),
            kind: false,
        }
    }
}

/// Both sides' headings on each of their lines (`lines`, without the
/// scrollbar's gutter and with it), side `index` headed `labels[index]`
/// for what `sides[index]` holds ([`side_heading`]), where both are drawn:
/// both read their kind, "· PDF" or a format's caption, or neither, the
/// pair dropping it together where either side's line does not hold its
/// own, so a reader across the two compares like with like. Each side once
/// dropped its kind alone, and in a 461 × 490 window two paged PDFs
/// were headed "Before" beside "After · PDF", "Before · PDF" the wider.
fn pair_headings(
    labels: [&str; 2],
    sides: [&rich_preview::Side; 2],
    lines: [Pixels; 2],
    window: &Window,
) -> [[Heading; 2]; 2] {
    let headings = [0, 1].map(|index| {
        lines.map(|line| side_heading(index, labels[index], sides[index], line, true, window))
    });
    let kinds = [0, 1].map(|at| headings.iter().all(|side| side[at].kind));
    [0, 1].map(|index| {
        [0, 1].map(|at| {
            if kinds[at] {
                headings[index][at].clone()
            } else {
                side_heading(index, labels[index], sides[index], lines[at], false, window)
            }
        })
    })
}

/// The room the first line of a side's header leaves its open control, on
/// lines `line` wide ([`pair_line`]): the line less its semibold heading,
/// `heading` wide, and the `gap_2` after it on its device pixel.
fn header_room(line: Pixels, heading: Pixels, window: &Window) -> Pixels {
    line - window.pixel_snap(window.rem_size() * 0.5) - heading
}

/// The width of a line of both paged sides' headers in a comparison `width`
/// wide, on lines that keep the scrollbar's gutter where `scrolls`
/// (`system_preview::side_line_room`), as layout gives the narrower side on
/// this display's device pixels: each side is laid out in its own frame
/// (`system_preview::header_ends_for_room`), whose bounds GPUI rounds at
/// their edges, halves toward zero, so in a comparison an odd number of
/// device pixels wide Before's frame is a device pixel narrower than
/// After's; then less its right rule (at least a device pixel), the
/// header's `px_2` either side and the gutter, each on its device pixel.
/// Both headers are held to this width (`render_comparison`), so the two
/// wrap alike, each frame laid out on its whole device pixels
/// (`in_frame::on_whole_device_pixels`): the narrower frame once reached
/// layout through floats (243 device pixels as 242.99998 at scale 1.2), so
/// a run that filled the line wrapped there, and the line was a device
/// pixel narrower to keep it whole.
/// Laid out each in its own side, Before's run of a 3-page document wrapped
/// where After's identical run held one line, at 13 pt and scale 1.2 in
/// 147 px sides, and the side measured unrounded, less 1 px and an
/// unsnapped half rem either side, was up to a device pixel wider than
/// drawn, so a header said to fit its lines wrapped.
fn pair_line(width: Pixels, quick: bool, scrolls: bool, window: &Window) -> Pixels {
    let scale = window.scale_factor();
    let device = |length: Pixels| (f32::from(length) * scale).round();
    let sides = if quick { 1. } else { 2. };
    let side = (device(width) / sides).floor();
    let rule = window.pixel_snap(px(1.)).max(px(1. / scale));
    let gutter = if scrolls {
        window.pixel_snap(crate::tags::ScrollCue::gutter())
    } else {
        Pixels::ZERO
    };
    let inset = window.pixel_snap(window.rem_size() * 0.5);
    px((side - device(rule) - device(inset) * 2. - device(gutter)).max(0.) / scale)
}

/// The readings a page number of `count` pages is measured at, whole
/// ("Page 1 / 3") and short ("1/3"): the count's digits all one digit, for
/// each of the ten, so the widest of them is as wide as any page's reading
/// even where a proportional font's digits differ in width. Measured at the
/// count alone ("Page 111 / 111"), the labelled form was narrower than
/// another page's ("Page 108 / 111") and that page's was cut.
fn page_number_readings(count: usize) -> [Vec<String>; 2] {
    let digits = count.to_string().len();
    let numbers: Vec<String> = ('0'..='9')
        .map(|digit| digit.to_string().repeat(digits))
        .collect();
    [
        numbers
            .iter()
            .map(|number| format!("Page {number} / {number}"))
            .collect(),
        numbers
            .iter()
            .map(|number| format!("{number}/{number}"))
            .collect(),
    ]
}

/// The widths of side `index`'s page number labelled and short at their
/// widest for `count` pages ([`page_number_readings`]), as layout places
/// them on this display's device pixels ([`crate::placed_button_width`]),
/// from their texts laid out once per font and size for each number of
/// digits, which alone sets the readings: the twenty readings were once
/// built and joined on every frame. Drawn at their unrounded sum, "Page 1 /
/// 3" was cut, "Page 1…", at 13 pt and scale 1.2.
fn page_number_widths(index: usize, count: usize, window: &Window) -> (Pixels, Pixels) {
    let cache = if index == 0 {
        &BEFORE_PAGE_NUMBER
    } else {
        &AFTER_PAGE_NUMBER
    };
    let digits = count.checked_ilog10().map_or(1, |power| power as usize + 1);
    let key = crate::views::LineKey::current(window);
    let (whole, short) = cache.with(|cache| {
        let mut cache = cache.borrow_mut();
        match &*cache {
            Some((laid_out, measured, texts)) if *laid_out == key && *measured == digits => *texts,
            _ => {
                #[cfg(test)]
                PAGE_NUMBER_READINGS_BUILT.with(|built| built.set(built.get() + 1));
                let widest = |readings: &[String]| {
                    readings
                        .iter()
                        .map(|reading| {
                            crate::views::line_widths(
                                &[(reading, appearance::ui_text(12.))],
                                window,
                            )
                        })
                        .fold(Pixels::ZERO, Pixels::max)
                };
                let [whole, short] = page_number_readings(count);
                let texts = (widest(&whole), widest(&short));
                *cache = Some((key, digits, texts));
                texts
            }
        }
    });
    (
        crate::placed_button_width(whole, false, window),
        crate::placed_button_width(short, false, window),
    )
}

/// The form a side's own header takes ([`header_layout`]), in the order a
/// side gives way to as it narrows: never back to a wider one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum HeaderForm {
    /// Every control labelled: the open control, Read page text (or Retry
    /// page), Previous, Next, the page number, the link and the zoom run.
    Full,
    /// Previous, the page number and Next, one zoom menu and one "more" menu
    /// holding Read page text (or Retry page), the link and the open control,
    /// on one line beneath the heading or beside it.
    Compact,
    /// Previous, the page number and Next, and one "more" menu that holds
    /// the zooms too, its lines a quarter rem apart and from its edges.
    Narrowest,
}

#[cfg(test)]
thread_local! {
    /// The form each side's header was last drawn in, by side, for a test.
    pub(crate) static DRAWN_FORMS: std::cell::RefCell<std::collections::BTreeMap<usize, HeaderForm>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
    /// Each side's header as it was last laid out, by side, for a test.
    pub(crate) static DRAWN_LAYOUTS: std::cell::RefCell<std::collections::BTreeMap<usize, HeaderLayout>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
    /// The room each side's header was last laid out for, by side, for a
    /// test.
    pub(crate) static DRAWN_ROOMS: std::cell::RefCell<std::collections::BTreeMap<usize, Pixels>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
    /// Each side's heading's bounds in every frame painted, by side, in
    /// order, while a test records them (`Some`): GPUI's test harness draws
    /// a window made dirty before the test's own draw, so a frame drawn
    /// once is seen only here.
    pub(crate) static DRAWN_HEADINGS: HeadingFrames = const { std::cell::RefCell::new(None) };
}

/// Each side's heading's bounds, by side, in the order its frames were
/// painted ([`DRAWN_HEADINGS`]).
#[cfg(test)]
type HeadingFrames = std::cell::RefCell<Option<Vec<(usize, Bounds<Pixels>)>>>;

/// What a side's full header draws on its lines, each as wide as it is
/// drawn at its widest reading, in the header's order.
pub(crate) struct FullHeader {
    pub(crate) heading: Pixels,
    /// The open control in the form its first line gives it, where offered.
    pub(crate) open: Option<Pixels>,
    /// Read page text and Retry page, at the wider.
    pub(crate) page: Pixels,
    /// The page number labelled.
    pub(crate) number: Pixels,
    /// The link, where the side has a partner to link to.
    pub(crate) link: Option<Pixels>,
    /// Fit, −, + and the zoom, one run ([`zoom_run_width`]).
    pub(crate) zooms: Pixels,
}

/// What a side's header draws, measured for lines `line` wide: the full
/// form's parts, the page number's short reading, the compact zoom menu, and
/// the full form's line about different page counts where it shows, its
/// width and height.
pub(crate) struct HeaderParts {
    pub(crate) line: Pixels,
    pub(crate) full: FullHeader,
    pub(crate) short: Pixels,
    pub(crate) zoom_menu: Pixels,
    pub(crate) counts: Option<(Pixels, Pixels)>,
}

/// How a side's header is laid out ([`header_layout`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HeaderLayout {
    pub(crate) form: HeaderForm,
    /// Whether its lines leave the scrollbar's gutter: only where even the
    /// narrowest form is taller than its room, so it scrolls.
    pub(crate) scrolls: bool,
    /// The reading the page number takes, the same on both sides of a
    /// comparison ([`pair_layouts`]).
    pub(crate) reading: NumberReading,
    /// The width the page number is given in a compact form: as wide as the
    /// reading it takes ([`NumberReading::width`]).
    pub(crate) number: Pixels,
    /// The header's height as its lines lay it out.
    pub(crate) height: Pixels,
}

/// The reading a side's page number takes in a compact form's run
/// ([`number_room`]), in the order it gives way as a side narrows: the
/// full form always labels it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum NumberReading {
    /// "Page 1 / 3".
    Labelled,
    /// "1/3".
    Short,
    /// Its page icon, its reading its tooltip.
    Icon,
}

impl NumberReading {
    /// The width the page number is given to take this reading
    /// (`HeaderControl::button`): its labelled or short reading's, or none,
    /// where it draws its icon in its square.
    fn width(self, parts: &HeaderParts) -> Pixels {
        match self {
            Self::Labelled => parts.full.number,
            Self::Short => parts.short,
            Self::Icon => Pixels::ZERO,
        }
    }
}

/// The most lines a full header may take: beyond them, a side gets the
/// compact form.
const FULL_LINES: usize = 3;

/// The vertical rhythm of a header in `form`: the padding above and beneath
/// its lines, and the gap between them, each a whole device pixel as layout
/// snaps it; `gap_2` (half a rem) apart and around, but a quarter rem
/// between the narrowest form's lines and above and beneath them, which
/// still keeps the 3 px ring of a control on its first or last line inside
/// the header. Half a rem there, three lines (a heading and two of controls)
/// took 110 px at 13 pt in the 88 px side a 461 × 490 window gives, more
/// than the 102 px the page's minimum leaves the header, and 152 px at 18 pt
/// in 640 × 680, more than its 144 px; with half a rem above alone, 101.67 px
/// at scale 1.2, more than its 100.83 px there.
fn rhythm(form: HeaderForm, window: &Window) -> (Pixels, Pixels, Pixels) {
    let half = window.pixel_snap(window.rem_size() * 0.5);
    let quarter = window.pixel_snap(window.rem_size() * 0.25);
    if form == HeaderForm::Narrowest {
        (quarter, quarter, quarter)
    } else {
        (half, half, half)
    }
}

/// The scaled 28 px control's square as layout places it, on its device
/// pixel, halves toward zero: Previous, Next, the "more" menu and a
/// control drawn as its icon.
fn square(window: &Window) -> Pixels {
    window.pixel_snap(appearance::ui_size(28.))
}

/// One part of a header's wrapping row, as layout places it: one control or
/// text `width` wide and `height` tall, or a run of controls one line tall
/// that moves to the next line whole and wraps within only where it is wider
/// than a line, as the compact forms' controls do.
enum Part {
    One(Pixels, Pixels),
    Run(Vec<Pixels>),
}

/// The height `parts` take wrapped as the header's flex row wraps them, in
/// lines `line` wide and `gap_2` apart across, laid out in `form`'s rhythm
/// ([`rhythm`]), each control as tall as the scaled 28 px control. Widths
/// are compared on device pixels, as layout places them.
fn wrapped_height(parts: &[Part], line: Pixels, form: HeaderForm, window: &Window) -> Pixels {
    let (top, bottom, between) = rhythm(form, window);
    let across = window.pixel_snap(window.rem_size() * 0.5);
    let row = square(window);
    let on_device = |length| crate::image_compare::on_device_pixel(length, window);
    // The run's lines, each control placed greedily as a wrapping row does.
    let run_lines = |widths: &[Pixels]| {
        let mut lines = 0usize;
        let mut used: Option<Pixels> = None;
        for &width in widths {
            used = match used {
                Some(used) if on_device(used + across + width) <= line => {
                    Some(used + across + width)
                }
                _ => {
                    lines += 1;
                    Some(width)
                }
            };
        }
        lines.max(1)
    };
    let mut heights: Vec<Pixels> = Vec::new();
    let mut used: Option<Pixels> = None;
    for part in parts {
        let (width, height, whole) = match part {
            Part::One(width, height) => ((*width).min(line), on_device(*height), true),
            Part::Run(widths) => {
                let width = widths.iter().fold(Pixels::ZERO, |run, width| run + *width)
                    + across * widths.len().saturating_sub(1) as f32;
                if on_device(width) <= line {
                    (width, row, true)
                } else {
                    // As wide as its line, so nothing lies beside it.
                    let lines = run_lines(widths) as f32;
                    (line, row * lines + between * (lines - 1.), false)
                }
            }
        };
        used = match used {
            Some(used) if whole && on_device(used + across + width) <= line => {
                if let Some(last) = heights.last_mut() {
                    *last = (*last).max(height);
                }
                Some(used + across + width)
            }
            _ => {
                heights.push(height);
                Some(width)
            }
        };
    }
    let lines = heights.len().saturating_sub(1) as f32;
    heights
        .iter()
        .fold(top + bottom + between * lines, |sum, height| sum + *height)
}

/// Whether the full form fits `parts`: every part, labelled, fits a line of
/// the header to itself, and they wrap, as the header's flex row wraps them,
/// `gap_2` apart, into at most [`FULL_LINES`] lines.
fn full_fits(parts: &HeaderParts, window: &Window) -> bool {
    let widths = &parts.full;
    let row = square(window);
    let full = [
        Some(widths.heading),
        widths.open,
        Some(widths.page),
        Some(row),
        Some(row),
        Some(widths.number),
        widths.link,
        Some(widths.zooms),
    ];
    let gap = window.pixel_snap(window.rem_size() * 0.5);
    let on_device = |length| crate::image_compare::on_device_pixel(length, window);
    let mut lines = 0;
    let mut used: Option<Pixels> = None;
    for part in full.into_iter().flatten() {
        if on_device(part) > parts.line {
            return false;
        }
        used = match used {
            Some(used) if on_device(used + gap + part) <= parts.line => Some(used + gap + part),
            _ => {
                lines += 1;
                Some(part)
            }
        };
    }
    lines <= FULL_LINES
}

/// The controls of `form`'s run, in order, the page number `number` wide
/// (its icon's square where that is narrower): Previous, the page number and
/// Next, then the compact form's zoom menu, then the "more" menu.
fn run_widths(
    form: HeaderForm,
    parts: &HeaderParts,
    number: Pixels,
    window: &Window,
) -> Vec<Pixels> {
    let row = square(window);
    let number = number.max(row);
    let mut run = vec![row, number, row];
    if form == HeaderForm::Compact {
        run.push(parts.zoom_menu);
    }
    run.push(row);
    run
}

/// The reading the page number takes in `form`'s run, whose width chooses
/// its form ([`HeaderControl::button`]): the widest of its readings, labelled
/// or short, with which the whole run, Previous, the page number, Next and
/// the menus, fits one line, and otherwise its icon. As a side narrows it gives
/// way in one order, labelled, short, then its icon, and stays its icon as
/// the run wraps. Chosen as the reading that cost the header no line its
/// icon would not, it went from short to its icon and back to short again
/// and again as a side narrowed, whenever a wrap of the run left it room.
fn number_room(form: HeaderForm, parts: &HeaderParts, window: &Window) -> NumberReading {
    let gap = window.pixel_snap(window.rem_size() * 0.5);
    let fits = |reading: NumberReading| {
        let run = run_widths(form, parts, reading.width(parts), window);
        let width =
            run.iter().fold(Pixels::ZERO, |run, width| run + *width) + gap * (run.len() - 1) as f32;
        crate::image_compare::on_device_pixel(width, window) <= parts.line
    };
    [NumberReading::Labelled, NumberReading::Short]
        .into_iter()
        .find(|reading| fits(*reading))
        .unwrap_or(NumberReading::Icon)
}

/// The height of a header in `form` drawing `parts`, its page number
/// `number` wide where compact ([`wrapped_height`]).
fn header_height(form: HeaderForm, parts: &HeaderParts, number: Pixels, window: &Window) -> Pixels {
    let full = &parts.full;
    let row = square(window);
    let mut drawn = vec![Part::One(full.heading, row)];
    if form == HeaderForm::Full {
        let controls = [
            full.open,
            Some(full.page),
            Some(row),
            Some(row),
            Some(full.number),
            full.link,
            Some(full.zooms),
        ];
        drawn.extend(
            controls
                .into_iter()
                .flatten()
                .map(|width| Part::One(width, row)),
        );
        drawn.extend(parts.counts.map(|(width, height)| Part::One(width, height)));
    } else {
        // A number of no width draws its icon, in the 28 px square.
        drawn.push(Part::Run(run_widths(form, parts, number, window)));
    }
    wrapped_height(&drawn, parts.line, form, window)
}

/// Whether the compact form's run, Previous, the page number (short at
/// least), Next, the zoom menu and the "more" menu, fits one line: the
/// compact form is taken only there, never wrapping.
fn compact_fits(parts: &HeaderParts, window: &Window) -> bool {
    let gap = window.pixel_snap(window.rem_size() * 0.5);
    let controls = run_widths(HeaderForm::Compact, parts, parts.short, window);
    let width = controls
        .iter()
        .fold(Pixels::ZERO, |run, width| run + *width)
        + gap * (controls.len() - 1) as f32;
    crate::image_compare::on_device_pixel(width, window) <= parts.line
}

/// The form of a side's header by width alone: full where [`full_fits`];
/// compact where [`compact_fits`]; narrowest otherwise. Each test holds at
/// every wider line wherever it holds, so a side narrowing never returns to
/// a wider form.
fn width_form(parts: &HeaderParts, window: &Window) -> HeaderForm {
    if full_fits(parts, window) {
        HeaderForm::Full
    } else if compact_fits(parts, window) {
        HeaderForm::Compact
    } else {
        HeaderForm::Narrowest
    }
}

/// How a side's header is laid out, from what it draws on lines that keep
/// no scroll gutter (`unscrolled`) and on lines that keep it (`scrolled`),
/// and `room`, the most height its region holds without scrolling
/// (`system_preview::header_ends_for_room`): the form its width chooses
/// ([`width_form`]), or `at_least` where that is narrower, where it fits the
/// room, or the first narrower one that does, on lines with no gutter; and
/// only where even the narrowest is taller than the room, the narrowest on
/// lines less the gutter, scrolling. The compact form is tried only where its
/// run fits one line ([`compact_fits`]): a wrapping compact form is skipped
/// for the narrowest, so a short room never brings a compact header the width
/// would not. It is a function of the side's drawn width, its room, the text
/// size and the scale alone (and of `at_least`, its partner's form, which is
/// as well: [`pair_layouts`]), so at one size every frame lays the header out
/// alike, and a header that fits its room keeps no gutter. A side too narrow
/// for its whole header once got a header region that scrolled: at 18 pt in
/// a 461 × 490 window its controls out of view were out of reach for
/// assistive technology until scrolled, Read page text's accessible centre
/// lay over the page, and its messages were cut; and lines that always kept
/// the gutter put Previous and Next on separate lines in its 88 to 123 px
/// sides, taller than any room.
pub(crate) fn header_layout(
    unscrolled: &HeaderParts,
    scrolled: &HeaderParts,
    room: Pixels,
    at_least: HeaderForm,
    window: &Window,
) -> HeaderLayout {
    let laid_out = |form, parts: &HeaderParts, scrolls| {
        let reading = if form == HeaderForm::Full {
            NumberReading::Labelled
        } else {
            number_room(form, parts, window)
        };
        let number = reading.width(parts);
        HeaderLayout {
            form,
            scrolls,
            reading,
            number,
            height: header_height(form, parts, number, window),
        }
    };
    let widest = width_form(unscrolled, window).max(at_least);
    let compact = compact_fits(unscrolled, window);
    [HeaderForm::Full, HeaderForm::Compact, HeaderForm::Narrowest]
        .into_iter()
        .filter(|form| *form >= widest && (*form != HeaderForm::Compact || compact))
        .map(|form| laid_out(form, unscrolled, false))
        .find(|layout| layout.height <= room)
        .unwrap_or_else(|| laid_out(HeaderForm::Narrowest, scrolled, true))
}

/// Both sides' header layouts in the frame drawn, from what each draws
/// (`None` for a side without pages or not shown) and the room each side
/// leaves its header: one form, the narrower of the two sides' own
/// ([`header_layout`]), so their headings lie level, in the same rhythm
/// ([`rhythm`]), and both show the same controls. A narrower form fits a
/// side's width wherever a wider one does, and is laid out within its room
/// by that side's own steps, so each side keeps a header that fits; and
/// should a side's room still take it narrower, the pair is laid out again
/// at that form, so they never differ. In that form both draw the page
/// number in one reading too, the narrower of the two sides' own
/// ([`number_room`]): labelled only where both sides' labelled readings
/// keep their runs on one line, short only where both short ones do, and
/// otherwise both draw its icon, so the two runs lie on as many lines and
/// the pages lie level. A narrower reading never lengthens a run, so each
/// side's header still fits where it fit. Each side once chose its form
/// alone: beside an After of 111 pages, whose short page number is wider, a
/// Before of 3 took the compact form while After took the narrowest in 234
/// to 260 px sides at 13 pt, After's heading 3.5 px higher at scale 2, and
/// the full form beside the compact one at 292 to 304 px; and then its page
/// number alone, so at scale 1.2 in 156.5 px sides Before drew "1/3" while
/// After drew its icon, and the pages lay 38 device pixels apart.
fn pair_layouts(
    parts: [Option<&[HeaderParts; 2]>; 2],
    rooms: [Pixels; 2],
    window: &Window,
) -> [Option<HeaderLayout>; 2] {
    let mut form = HeaderForm::Full;
    let mut layouts = loop {
        let layouts = [0, 1].map(|at| {
            parts[at].map(|parts| header_layout(&parts[0], &parts[1], rooms[at], form, window))
        });
        // Each side's form is at least `form`, so the narrower of the two
        // is `form` only where both took it.
        let narrower = layouts
            .iter()
            .flatten()
            .map(|layout| layout.form)
            .max()
            .unwrap_or(form);
        if narrower == form {
            break layouts;
        }
        form = narrower;
    };
    let narrower = layouts.iter().flatten().map(|layout| layout.reading).max();
    for (layout, parts) in layouts.iter_mut().zip(parts) {
        if let (Some(layout), Some(parts), Some(reading)) = (layout, parts, narrower)
            && layout.reading != reading
        {
            let parts = &parts[usize::from(layout.scrolls)];
            layout.reading = reading;
            layout.number = reading.width(parts);
            layout.height = header_height(layout.form, parts, layout.number, window);
        }
    }
    layouts
}

/// What both sides of a comparison draw in their headers in one frame
/// ([`side_parts`]), measured once for the frame, and the pair's layouts
/// ([`pair_layouts`]), laid out once for the rooms the frame gives: the
/// first side drawn lays both out and the second takes them, its room the
/// one the first found for it ([`crate::system_preview::HeaderRoom::beside`]).
struct Pair {
    parts: [Option<SideParts>; 2],
    /// Whether a click on each side's open control answered, which takes the
    /// line saying why from that side's room.
    answered: [bool; 2],
    laid_out: std::cell::Cell<Option<PairLaidOut>>,
}

/// The rooms a [`Pair`] was laid out for, by side, and its layouts.
type PairLaidOut = ([Pixels; 2], [Option<HeaderLayout>; 2]);

impl Pair {
    /// Both sides' layouts, for side `index`'s `room` and its partner's,
    /// beside it.
    fn layouts(
        &self,
        room: crate::system_preview::HeaderRoom,
        index: usize,
        window: &Window,
        cx: &App,
    ) -> [Option<HeaderLayout>; 2] {
        let mut rooms = [room.most; 2];
        rooms[1 - index] = room.beside(self.answered[1 - index], window, cx);
        if let Some((laid_out, layouts)) = self.laid_out.get()
            && laid_out == rooms
        {
            return layouts;
        }
        let parts = self.parts.each_ref().map(Option::as_ref);
        let layouts = pair_layouts(parts, rooms, window);
        self.laid_out.set(Some((rooms, layouts)));
        layouts
    }
}

/// The open control's width as layout places it, in the form the heading's
/// line leaves it, on lines `line` wide, for a side that offers `offer`.
fn open_width(
    offer: Option<crate::system_preview::Offer>,
    line: Pixels,
    heading: &Heading,
    window: &Window,
) -> Option<Pixels> {
    offer.map(|offer| {
        if offer.form(header_room(line, heading.width, window), window)
            == crate::system_preview::Form::Labelled
        {
            offer.placed_width(window)
        } else {
            square(window)
        }
    })
}

/// The full form's line about different page counts, where `document`'s
/// pages are linked to a partner `other` of another count.
fn counts_line(document: &Document, other: &rich_preview::Side, quick: bool) -> Option<String> {
    let linked = document.linked.load(Ordering::Relaxed);
    other
        .pdf
        .as_ref()
        .filter(|other| !quick && linked && other.count != document.count)
        .map(|other| {
            format!(
                "Different page counts: {} / {}. Each side stops at its last page.",
                document.count, other.count
            )
        })
}

/// What a paged side's header draws on its lines without the scrollbar's
/// gutter and with it ([`header_layout`]).
type SideParts = [HeaderParts; 2];

/// What side `index` draws in its header beside `other` ([`SideParts`]),
/// headed `headings` on lines `lines` wide without the scrollbar's gutter
/// and with it ([`pair_line`]), or nothing for a side without pages, each
/// control as wide as layout places it on this display's device pixels.
/// Both sides' are measured once a frame and laid out as a pair ([`Pair`]),
/// so the two take one form and their headings lie level, and a side
/// without pages takes its partner's rhythm from them
/// ([`render_comparison`]).
fn side_parts(
    index: usize,
    (side, other): (&rich_preview::Side, &rich_preview::Side),
    quick: bool,
    (lines, headings): ([Pixels; 2], [Heading; 2]),
    window: &Window,
) -> Option<SideParts> {
    let document = side.pdf.as_ref()?;
    let offer = side.system_preview.filter(|_| side.captured.is_some());
    // Read page text and Retry page take the wider's width
    // (`render_comparison`).
    let slot = page_control_width(window);
    let (labelled, short) = page_number_widths(index, document.count, window);
    // At its wider reading, as it is drawn (`HeaderControl::placed`).
    let link = (!quick && other.pdf.is_some()).then(|| link_width(window));
    let counts = counts_line(document, other, quick);
    let parts = [0, 1].map(|at| HeaderParts {
        line: lines[at],
        full: FullHeader {
            heading: headings[at].width,
            open: open_width(offer, lines[at], &headings[at], window),
            page: slot,
            number: labelled,
            link,
            zooms: zoom_run_width(window),
        },
        short,
        zoom_menu: zoom_menu_width(window),
        counts: counts.as_ref().map(|text| {
            let size = appearance::ui_text(10.);
            let width = crate::views::line_widths(&[(text.as_str(), size)], window).min(lines[at]);
            let lines = crate::image_compare::wrapped_lines(text, size, width, window);
            (
                width,
                crate::image_compare::text_line(size, window) * lines as f32,
            )
        }),
    });
    Some(parts)
}

/// Fit, −, + and the zoom at its widest reading ("400%"), `gap_2` apart,
/// as layout places the full header's zoom run on this display's device
/// pixels: each button its label's width with its insets, at least its
/// 28 px square ([`crate::placed_button_width`]), each gap on its device
/// pixel, and the level as wide as its widest reading
/// ([`zoom_level_width`]).
fn zoom_run_width(window: &Window) -> Pixels {
    let texts = header_texts(window);
    let gap = window.pixel_snap(window.rem_size() * 0.5);
    let buttons = texts
        .zooms
        .map(|text| crate::placed_button_width(text, false, window));
    crate::image_compare::on_device_pixel(
        buttons
            .into_iter()
            .fold(zoom_level_width(window) + gap * 3., |run, button| {
                run + button
            }),
        window,
    )
}

/// The compact header's zoom menu labelled at its widest reading, whatever
/// the zoom ([`HeaderTexts::zoom_menu`]), as layout places it on this
/// display's device pixels: the label's control
/// ([`crate::placed_button_width`]), then the shared `button` helper's 6 px
/// gap and the kit's 0.875 rem caret, each on its device pixel.
fn zoom_menu_width(window: &Window) -> Pixels {
    crate::image_compare::on_device_pixel(
        crate::placed_button_width(header_texts(window).zoom_menu, false, window)
            + window.pixel_snap(appearance::ui_size(6.))
            + window.pixel_snap(window.rem_size() * 0.875),
        window,
    )
}

/// A side's message line where its page would be: a wait, a failure, a
/// locked or absent side. It shows every line it wraps to that its room
/// holds, and where it needs more, the whole lines that room holds, the
/// last ending in an ellipsis, at least one; its accessible name is always
/// the whole sentence, read as static text, which tests find as `id`.
/// Drawn whole in a page's room that scrolls, a long failure once ran past
/// the side's foot at 18 pt in a 461 × 490 window and was cut mid-line.
fn message_line(id: String, text: impl Into<SharedString>, color: Option<u32>) -> AnyElement {
    let text: SharedString = text.into();
    div()
        .size_full()
        .flex()
        .flex_col()
        .child(crate::in_frame::in_frame(move |bounds, window, _| {
            let size = window.text_style().font_size.to_pixels(window.rem_size());
            let line = crate::image_compare::text_line(size, window);
            let natural =
                crate::image_compare::wrapped_lines(&text, size, bounds.size.width, window);
            let fit = ((bounds.size.height / line).floor().max(1.)) as usize;
            let clamp = (natural > fit).then_some(fit);
            let selector = id.clone();
            native_accessibility::static_text(
                div()
                    .id(SharedString::from(id))
                    .debug_selector(move || selector)
                    .when_some(color, |line, color| line.text_color(rgb(color)))
                    .child(
                        div()
                            .min_w_0()
                            .when_some(clamp, |line, lines| line.line_clamp(lines).text_ellipsis())
                            .child(text.clone()),
                    ),
                text,
            )
            .into_any_element()
        }))
        .into_any_element()
}

/// Read page text while its page is prepared: disabled, its tooltip saying
/// why as its accessible name does ([`PREPARING`]), and recorded as it is
/// drawn for side `index`'s tests (`system_preview::DRAWN_CONTROLS`), which
/// once held the enabled control's tooltip and name while the drawn control
/// said why it was unavailable.
fn preparing(read: Button, index: usize) -> Button {
    let tooltip = format!("Read page text · {PREPARING}");
    #[cfg(test)]
    crate::system_preview::DRAWN_CONTROLS.with_borrow_mut(|drawn| {
        if let Some(drawn) = drawn.get_mut(&format!("pdf-read-text-{index}")) {
            drawn.tooltip = Some(tooltip.clone());
        }
    });
    #[cfg(not(test))]
    let _ = index;
    read.disabled(true).tooltip(tooltip)
}

/// The comparison's sides, each with its header of page controls; `width`
/// is the comparison's in the frame it is drawn
/// (`rich_preview::render_comparison`), which those controls, the open
/// control among them, take their forms from.
pub(super) fn render_comparison<T: 'static>(
    preview: &rich_preview::Comparison,
    quick: bool,
    opener: crate::system_preview::Opener,
    width: Pixels,
    window: &Window,
    cx: &mut Context<T>,
) -> AnyElement {
    let colors = palette(cx);
    let (muted, border) = (colors.muted, colors.border);
    let inset = window.rem_size() * 0.5;
    // A line of either side's header to itself, without the gutter a
    // scrollbar takes and with it, as layout gives the narrower side, to
    // which both are held ([`pair_line`]): the header keeps the gutter only
    // where even its narrowest form is taller than its room, so it scrolls
    // ([`header_layout`]). A labelled control wider than its line draws its
    // icon or a shorter reading rather than be cut at the side's edge, its
    // whole label in its tooltip and its accessible name the one it has
    // labelled.
    let lines = [false, true].map(|scrolls| pair_line(width, quick, scrolls, window));
    let sides = [
        ("Before", &preview.old, &preview.new),
        (
            if quick { "Source" } else { "After" },
            &preview.new,
            &preview.old,
        ),
    ];
    let drawn_side = |index: usize| !quick || index == 1;
    // Each shown side's open control, and why its last click opened nothing.
    let mut opens = [0, 1].map(|index| {
        let (label, side, _) = sides[index];
        match (side.system_preview, &side.captured) {
            (Some(offer), Some(bytes)) if drawn_side(index) => Some((
                opener.target(
                    ("pdf-system", index),
                    label,
                    bytes,
                    &side.name,
                    "PDF",
                    offer,
                ),
                offer,
            )),
            _ => None,
        }
    });
    let mut answers = opens
        .each_ref()
        .map(|open| open.as_ref().and_then(|(target, _)| target.answered(cx)));
    // Each side's heading on each line ([`side_heading`]), both reading
    // their kind or neither where both are drawn ([`pair_headings`]).
    let headings = if quick {
        [0, 1].map(|index| {
            let (label, side, _) = sides[index];
            lines.map(|line| side_heading(index, label, side, line, true, window))
        })
    } else {
        pair_headings(
            sides.map(|(label, _, _)| label),
            sides.map(|(_, side, _)| side.as_ref()),
            lines,
            window,
        )
    };
    // What both sides' headers draw, measured once for the frame, laid out
    // as a pair in it ([`Pair`]).
    let pair = std::rc::Rc::new(Pair {
        parts: [0, 1].map(|index| {
            let (_, side, other) = sides[index];
            drawn_side(index)
                .then(|| {
                    side_parts(
                        index,
                        (side, other),
                        quick,
                        (lines, headings[index].clone()),
                        window,
                    )
                })
                .flatten()
        }),
        answered: answers.each_ref().map(Option::is_some),
        laid_out: std::cell::Cell::new(None),
    });
    div().size_full().flex().min_w_0().children(sides.into_iter().enumerate().filter(|(index, _)| drawn_side(*index)).map(|(index,(label, side, other))| {
        let headings = headings[index].clone();
        // The header's padding, which its floor in `header_ends` counts.
        let padding = rems(0.5);
        // The heading's line is as tall as the scaled 28 px controls, so an
        // absent side's header aligns with its partner's; the header's
        // padding beneath and its rule end it beneath the line saying why a
        // click opened nothing (`system_preview::header_ends`).
        // The header's `px_2` on its device pixel, as layout places it.
        let inset_drawn = window.pixel_snap(inset);
        let header_for = move |heading: &Heading, line: Pixels, form: HeaderForm| {
            // Tests read what the heading reads after its selector and a
            // `/`, a selector only a test build sets (`debug_selector` is a
            // no-op in release), so only a test build copies the text for it.
            let text = div().min_w_0().truncate();
            #[cfg(test)]
            let text = {
                let reads = heading.text.clone();
                text.debug_selector(move || format!("pdf-side-heading-{index}/{reads}"))
            };
            let caption = heading.caption.clone().map(|format| rich_preview::format_caption("pdf-side-format", index, format, muted));
            // The narrowest form's lines lie a quarter rem apart and from
            // the header's top and rule ([`rhythm`]).
            let tight = if form == HeaderForm::Narrowest { rems(0.25) } else { rems(0.5) };
            // Held to the pair's line, so both sides wrap alike: the side
            // laid out in the wider frame once kept on one line a run that
            // wrapped in its partner's ([`pair_line`]).
            let header = div().debug_selector(move || format!("pdf-side-header-{index}")).max_w(line + inset_drawn * 2.).flex().flex_wrap().items_center().gap_x(rems(0.5)).gap_y(tight).px(rems(0.5)).pt(tight).pb(tight);
            // The heading's bounds in every frame painted, its first child,
            // for a test that reads a frame the test harness draws itself.
            #[cfg(test)]
            let header = header.on_children_prepainted(move |children, _, _| {
                DRAWN_HEADINGS.with_borrow_mut(|frames| {
                    if let (Some(frames), Some(heading)) = (frames, children.first()) {
                        frames.push((index, *heading));
                    }
                });
            });
            // Held to its line: in the first frame a header scrolls in, as a
            // box shortens, its lines leave the gutter while its region still
            // lays them out as wide as before it overflowed
            // (`tags::ScrollCue`), and the heading drew whole into the gutter.
            header.child(div().debug_selector(move || format!("pdf-side-heading-{index}")).min_w_0().max_w(line).min_h(appearance::ui_size(28.)).flex().items_center().gap_2().font_weight(FontWeight::SEMIBOLD).child(text.child(heading.text.clone())).children(caption))
        };
        let end = crate::system_preview::HeaderEnd { inset, bottom: inset, right_rule: false, border };
        let name = if index == 0 { "pdf-side-0" } else { "pdf-side-1" };
        // Where even the narrowest form is taller than the room above the
        // page's minimum, as in the Stashes dialog's and the rewrite
        // review's small file boxes and at 18 pt in a 461 × 490 window, the
        // header scrolls, revealing the control Tab moves onto
        // (`system_preview::header_ends`), as the Stashes detail around it
        // does each of them.
        let held = crate::system_preview::SideHeader::new(name, if index == 0 { "pdf-side-0/scrollbar" } else { "pdf-side-1/scrollbar" }, opener.showing(), window, cx).within(&opener);
        let (answer, open) = (answers[index].take(), opens[index].take());
        // The open control in the form the heading's line leaves it.
        let offered = open.as_ref().map(|(_, offer)| *offer);
        let mut body = div().id(("pdf-scroll",index)).debug_selector(move || format!("pdf-scroll-{index}")).flex_1().min_h_0().min_w_0().overflow_scroll().p_3();
        let Some(document) = side.pdf.clone() else {
            // A side without pages has no page controls, so its header is
            // its heading and its open control, which keep the gutter only
            // where they are taller than their room.
            let body = body.child(message_line(format!("pdf-side-message-{index}"), side.error.clone().unwrap_or_else(|| if side.present {"PDF preview unavailable"} else {"No file on this side"}.into()), None));
            // Beside a side with pages, its partner's header as the pair
            // lays it out for that side's own room ([`Pair`]), whose rhythm
            // this header takes where it is the narrowest form, so the two
            // headings lie level: in the half rem of the other forms,
            // Before's heading lay 3.25 px below After's at 13 pt in a
            // 461 × 490 window.
            let (tracker, pair) = (held.clone(), pair.clone());
            let header = move |room: crate::system_preview::HeaderRoom, window: &mut Window, cx: &mut App| {
                let rhythm = match pair.layouts(room, index, window, cx)[1 - index] {
                    Some(HeaderLayout { form: HeaderForm::Narrowest, .. }) => HeaderForm::Narrowest,
                    _ => HeaderForm::Full,
                };
                let height = |heading: &Heading, line| {
                    let row = square(window);
                    let parts: Vec<Part> = std::iter::once(heading.width).chain(open_width(offered, line, heading, window)).map(|width| Part::One(width, row)).collect();
                    wrapped_height(&parts, line, rhythm, window)
                };
                let scrolls = height(&headings[0], lines[0]) > room.most;
                let at = usize::from(scrolls);
                let mut header = header_for(&headings[at], lines[at], rhythm);
                if let Some((target, offer)) = &open {
                    header = header.child(target.held_control(offer.form(header_room(lines[at], headings[at].width, window), window), &tracker, cx));
                }
                header.into_any_element()
            };
            return div().flex_1().min_w_0().h_full().flex().flex_col().child(crate::system_preview::header_ends_for_room(name, header, padding, held, vec![answer], end, body.into_any_element())).into_any_element();
        };
        let partner = if quick { None } else { other.pdf.clone() };
        let (page, zoom, error, cached) = {
            let mut state = document.state.lock().unwrap_or_else(|e| e.into_inner());
            let page = state.page;
            (page, state.zoom, state.error.clone(), state.cache.get(page))
        };
        // The page's own control holds one place at one width whatever the
        // page's state, so nothing after it moves while a page prepares:
        // Read page text once the page's text has arrived, Retry page where
        // the page failed, and in between Read page text disabled with its
        // reason. Present only once the text arrived, it once left its place
        // while the next page prepared, and Next, the page number and the
        // link moved left under the pointer, so a second click on Next
        // unlinked the pages. Both readings take the form and width of the
        // wider, Read page text, so a failure that brings Retry page moves
        // nothing either.
        let slot = page_control_width(window);
        // "1/3" where "Page 1 / 3" does not fit, and its page icon where
        // "1/3" does not either, its whole reading its tooltip; each form
        // measured at its widest for the count, and drawn that wide on every
        // page (`HeaderControl::button`).
        let (labelled, short) = page_number_widths(index, document.count, window);
        let linked = document.linked.load(Ordering::Relaxed);
        // Named for the state it shows, as the model's link is.
        let link_name = format!("{label}: {}", if linked {"Pages linked. Switch to independent pages"} else {"Pages independent. Link pages to this page"});
        // The full form's line about different page counts, where linked
        // pages differ in count.
        let counts = counts_line(&document, other, quick);
        let view = cx.entity().downgrade();
        let retry_page = {
            let (target, view) = (document.clone(), view.clone());
            move |window: &mut Window, cx: &mut App| {
                let mut state=target.state.lock().unwrap_or_else(|e|e.into_inner());state.error=None;state.pending=false;state.generation=state.generation.wrapping_add(1);drop(state);window.refresh();let _=view.update(cx,|_,cx|cx.notify());
            }
        };
        let flip_link = {
            let (target, partner, view) = (document.clone(), partner.clone(), view.clone());
            move |window: &mut Window, cx: &mut App| {
                let Some(partner) = &partner else { return };
                target.linked.store(!linked,Ordering::Relaxed); partner.linked.store(!linked,Ordering::Relaxed);
                if !linked { target.select(page,Some(partner)); }
                window.refresh(); let _=view.update(cx,|_,cx|cx.notify());
            }
        };
        let zoom_by = {
            let (target, view) = (document.clone(), view.clone());
            move |action: i32, window: &mut Window, cx: &mut App| {
                let mut state = target.state.lock().unwrap_or_else(|e|e.into_inner()); state.zoom=if action==0 {1.}else{(state.zoom * if action<0 {0.8}else{1.25}).clamp(0.25,4.)}; drop(state); window.refresh(); let _=view.update(cx,|_,cx|cx.notify());
            }
        };
        let zoom_name = move |action: i32| format!("{label}: {}",match action {-1=>"Zoom out",1=>"Zoom in",_=>"Fit page width"});
        // The page's own control's name, and while the page is prepared why
        // it is unavailable as well, as its tooltip says ([`preparing`]).
        let read_name = format!("{label}: read extracted text from page {}",page+1);
        let retry_name = format!("{label}: retry page {}",page+1);
        let level = format!("{:.0}%",zoom*100.);
        // The controls a menu of a compact form holds, whose focus it takes
        // where one of them held it as the form changed, and the menus whose
        // focus they take back (`SideHeader::track_for`).
        let fit = ElementId::from(("pdf-zoom", index*3+1));
        let zoom_controls = [fit.clone(), ElementId::from(("pdf-zoom", index*3)), ElementId::from(("pdf-zoom", index*3+2))];
        let zoom_menu_id = ElementId::from(("pdf-zoom-menu", index));
        let more_id = ElementId::from(("pdf-more", index));
        let page_controls = [ElementId::from(("pdf-read-text", index)), ElementId::from(("pdf-retry", index)), ElementId::from(("pdf-link", index)), ElementId::from(("pdf-system", index))];
        let failed = error.is_some();
        let (tracker, turn_view, shown, header_cached, pair) = (held.clone(), view.clone(), document.clone(), cached.clone(), pair.clone());
        let header = move |room: crate::system_preview::HeaderRoom, window: &mut Window, cx: &mut App| {
            let (held, view, document, cached) = (tracker, turn_view, shown, header_cached);
            // As wide as the pair measured it ([`link_width`]).
            let link = partner.as_ref().map(|_| HeaderControl::toggle(("pdf-link",index), format!("pdf-link-{index}"), LINK_READINGS, link_name.clone(), "link", linked, window).placed(link_width(window)));
            // The form and lines the room chooses from what each line draws,
            // with and without the gutter ([`side_parts`]), the narrower of
            // the two sides' forms ([`pair_layouts`]): a function of the
            // sides' width, their rooms, the text size and the scale alone,
            // so it never changes between frames at one size, nor as a page
            // prepares or turns ([`header_layout`]).
            let Some(layout) = pair.layouts(room, index, window, cx)[index] else { unreachable!("a side with pages is laid out") };
            #[cfg(test)]
            {
                DRAWN_FORMS.with_borrow_mut(|drawn| drawn.insert(index, layout.form));
                DRAWN_LAYOUTS.with_borrow_mut(|drawn| drawn.insert(index, layout));
                DRAWN_ROOMS.with_borrow_mut(|drawn| drawn.insert(index, room.most));
            }
            let at = usize::from(layout.scrolls);
            let line_room = lines[at];
            let mut header = header_for(&headings[at], line_room, layout.form);
            let mut turns = Vec::with_capacity(3);
            for (previous, disabled) in [(true,page == 0),(false,page+1 >= document.count)] {
                let (document, partner, view) = (document.clone(), partner.clone(), view.clone());
                let id = if previous {"pdf-back"} else {"pdf-forward"};
                let step = button((id,index), "", if previous {"chevron-left"} else {"chevron-right"}, false)
                    .debug_selector(move || format!("{id}-{index}"))
                    .accessibility_label(format!("{label}: {} page",if previous {"Previous"} else {"Next"})).disabled(disabled)
                    .on_click(move |_, window, cx| { document.select(if previous {page.saturating_sub(1)} else {page+1},partner.as_deref()); window.refresh(); let _=view.update(cx,|_,cx|cx.notify()); });
                turns.push(held.track((id,index), step, cx));
            }
            // The page number keeps the form a line to itself gives it in
            // the full form; in a compact one, the widest that costs no line
            // ([`number_room`]).
            let number_room = if layout.form == HeaderForm::Full { line_room } else { layout.number };
            let (target, linked_partner) = (document.clone(), partner.clone());
            let number = HeaderControl::new(("pdf-page-number",index), format!("pdf-page-number-{index}"), format!("Page {} / {}",page+1,document.count), format!("{label}: page {} of {}. Enter page number",page+1,document.count), "page", labelled)
                .short(format!("{}/{}",page+1,document.count), short).button(number_room)
                .on_click(move |_,window,cx|page_picker(target.clone(),linked_partner.clone(),label,window,cx));
            let number = held.track(("pdf-page-number",index), number, cx);
            let [back, forward]: [Button; 2] = turns.try_into().unwrap_or_else(|_| unreachable!("Previous and Next"));
            if layout.form == HeaderForm::Full {
                if let Some((target, offer)) = &open {
                    header = header.child(target.held_control(offer.form(header_room(lines[at], headings[at].width, window), window), &held, cx));
                }
                let page_control = if failed {
                    let retry = HeaderControl::new(("pdf-retry",index), format!("pdf-retry-{index}"), "Retry page", retry_name.clone(), "refresh", slot).labelled_with_icon().button(line_room);
                    let retry_page = retry_page.clone();
                    let retry = retry.on_click(move|_,window,cx| retry_page(window,cx));
                    held.track_for(("pdf-retry",index), std::slice::from_ref(&more_id), retry, window, cx)
                } else {
                    let read = |name: String| HeaderControl::new(("pdf-read-text",index), format!("pdf-read-text-{index}"), "Read page text", name, "file-text", slot).labelled_with_icon().button(line_room);
                    let read = match cached.clone() {
                        Some(prepared) => read(read_name.clone()).on_click(move|_,window,cx|read_page_text(prepared.clone(),label,page+1,window,cx)),
                        None => preparing(read(format!("{read_name} · {PREPARING}")), index),
                    };
                    held.track_for(("pdf-read-text",index), std::slice::from_ref(&more_id), read, window, cx)
                };
                // Its node recorded in tests as it is drawn, so they read the
                // state an AT-SPI client is given; outside tests the Button
                // as it is.
                use crate::native_accessibility::DrawnButton as _;
                header = header.child(page_control.node_recorded()).child(back).child(forward).child(number);
                if let Some(link) = link {
                    let flip_link = flip_link.clone();
                    let link = link.button(line_room).on_click(move |_,window,cx| flip_link(window,cx));
                    header = header.child(held.track(("pdf-link",index), link, cx));
                }
                // Fit, −, + and the zoom are one run, which moves to the next
                // line of the header whole rather than break, as the image
                // comparison's mode and zoom menus do; at 1480 × 973 a side
                // with no link once ended its first line with Fit and began
                // the next with − + 100%. The full form is chosen only where
                // a line holds the run ([`full_fits`]).
                let mut zooms = div().debug_selector(move || format!("pdf-zooms-{index}")).max_w_full().flex().flex_wrap().items_center().gap_2();
                for (action,text) in [(0i32,"Fit"),(-1,"−"),(1,"+")] {
                    let id = index*3+(action+1) as usize;
                    let zoom_by = zoom_by.clone();
                    let zoom = button(("pdf-zoom",id),text,"",false).debug_selector(move || format!("pdf-zoom-{id}")).accessibility_label(zoom_name(action)).on_click(move |_,window,cx| zoom_by(action,window,cx));
                    // Fit takes the zoom menu's focus where it held it as
                    // the form changed.
                    let zoom = if action == 0 { held.track_for(fit.clone(), std::slice::from_ref(&zoom_menu_id), zoom, window, cx) } else { held.track(("pdf-zoom",id), zoom, cx) };
                    zooms = zooms.child(zoom);
                }
                // At its widest reading, so a zoom never changes the run's
                // width.
                let widest = zoom_level_width(window);
                zooms = zooms.child(div().debug_selector(move || format!("pdf-zoom-level-{index}")).min_w(widest).whitespace_nowrap().text_size(appearance::ui_text(10.)).child(level.clone()));
                header = header.child(zooms);
                if let Some(counts) = counts {
                    header = header.child(div().text_size(appearance::ui_text(10.)).child(counts));
                }
                return header.into_any_element();
            }
            // Compact, Previous, the page number and Next, then the menus,
            // are one run, which moves to the next line of the header whole,
            // so they stay together on one line wherever they fit; where
            // they do not, the "more" menu wraps first, and the three split
            // only where they do not fit a line. Laid out one by one,
            // Previous could end the heading's line and Next begin the next.
            let between = if layout.form == HeaderForm::Narrowest { rems(0.25) } else { rems(0.5) };
            let mut run = div().debug_selector(move || format!("pdf-turns-{index}")).max_w_full().flex().flex_wrap().items_center().gap_x(rems(0.5)).gap_y(between).child(back).child(number).child(forward);
            let zoom_items = {
                let zoom_by = zoom_by.clone();
                move |menu: PopupMenu| [(0, "Fit page width"), (-1, "Zoom out"), (1, "Zoom in")].into_iter().fold(menu, |menu, (action, text)| {
                    let zoom_by = zoom_by.clone();
                    menu.item(PopupMenuItem::new(text).accessibility_label(zoom_name(action)).on_click(move |_, window, cx| zoom_by(action, window, cx)))
                })
            };
            let narrowest = layout.form == HeaderForm::Narrowest;
            if !narrowest {
                // The zoom group as one menu reading the zoom, as the image
                // comparison's toolbar folds its zooms: Fit page width, Zoom
                // out and Zoom in, named as the buttons they replace.
                let menu = HeaderControl::new(zoom_menu_id.clone(), format!("pdf-zoom-menu-{index}"), level.clone(), format!("{label}: Zoom, {level}"), "search", zoom_menu_width(window))
                    .menu("Fit page width, Zoom out or Zoom in").button(line_room);
                let zoom_items = zoom_items.clone();
                run = run.child(held.track_for(zoom_menu_id.clone(), &zoom_controls, menu, window, cx).app_menu(move |menu, _, _| zoom_items(menu)));
            }
            // Read page text (or Retry page), the link and the open control
            // as one "more" menu, its icon alone, as the model's views menu
            // is in a narrow side; the narrowest form folds the zooms into it
            // too, beneath the zoom it reads. Each item is named as the
            // control it replaces, and Read page text, while its page is
            // prepared, is unavailable for the reason it states.
            let page_choice = if failed {"Retry page"} else {"Read page text"};
            let choices = narrowest.then_some("Fit page width, Zoom out, Zoom in").into_iter()
                .chain([Some(page_choice), link.as_ref().map(|_| "Linked pages"), open.as_ref().map(|(_, offer)| offer.label())].into_iter().flatten())
                .collect::<Vec<_>>().join(", ");
            let (reading, name) = if narrowest {
                (format!("More · {level}"), format!("{label}: More page actions, zoom {level}"))
            } else {
                ("More".to_owned(), format!("{label}: More page actions"))
            };
            let more = HeaderControl::new(more_id.clone(), format!("pdf-more-{index}"), reading, name, "more", appearance::ui_size(28.))
                .menu(choices).button(px(0.));
            // The controls whose focus the menu takes as the form changes.
            let folded: Vec<ElementId> = if narrowest { zoom_controls.iter().cloned().chain([zoom_menu_id.clone()]).chain(page_controls.iter().cloned()).collect() } else { page_controls.to_vec() };
            let (cached, open) = (cached.clone(), open.clone());
            let has_link = link.is_some();
            let (retry_page, flip_link) = (retry_page.clone(), flip_link.clone());
            let more = held.track_for(more_id.clone(), &folded, more, window, cx).app_menu(move |menu, _, _| {
                let mut menu = menu;
                if narrowest {
                    menu = zoom_items(menu.label(format!("Zoom · {level}"))).separator();
                }
                let page_item = if failed {
                    let retry_page = retry_page.clone();
                    PopupMenuItem::new("Retry page").accessibility_label(retry_name.clone()).on_click(move |_, window, cx| retry_page(window, cx))
                } else if let Some(prepared) = cached.clone() {
                    PopupMenuItem::new("Read page text").accessibility_label(read_name.clone()).on_click(move |_, window, cx| read_page_text(prepared.clone(), label, page+1, window, cx))
                } else {
                    native_accessibility::detailed_item("Read page text", PREPARING).accessibility_label(format!("{read_name} · {PREPARING}"))
                };
                menu = menu.item(page_item);
                if has_link {
                    let flip_link = flip_link.clone();
                    menu = menu.item(PopupMenuItem::new("Linked pages").checked(linked).accessibility_label(link_name.clone()).on_click(move |_, window, cx| flip_link(window, cx)));
                }
                if let Some((target, _)) = &open {
                    menu = menu.item(target.menu_item());
                }
                menu
            });
            header.child(run.child(more)).into_any_element()
        };
        // The position's live region lies outside the header's region, which
        // can scroll it out of view and out of reach of assistive technology
        // while it is still being announced, in a 1 px box at the side's
        // top-left, in the header's padding, where no control or page lies:
        // laid over the whole side, it took AccessKit's hit tests, which try
        // later siblings first, from Fit, −, +, the page number and the page.
        let status = crate::native_accessibility::recorded(div().id(SharedString::from(format!("pdf-position-status-{index}"))).absolute().top_0().left_0().size(px(1.)).role(Role::Status).aria_label(format!("{label}: page {} of {}, {:.0} percent zoom",page+1,document.count,zoom*100.)).polite());
        let page_number = page + 1;
        let rendered = match side.system_preview.filter(|_| side.captured.is_some()) {
            Some(offer) => format!("Rendered page; {} opens the captured document.", offer.label()),
            None => "Rendered page.".into(),
        };
        if let Some(page) = cached && let Some(render) = &page.render {
            body = body.child(div().id(("pdf-rendered-page",index)).role(Role::Image).aria_label(format!("{label}: PDF page {} of {}. {rendered}",page_number,document.count)).w(relative(zoom)).min_w(px(100.)).aspect_ratio(page.width as f32/page.height as f32).bg(rgb(0xffffff)).child(crate::gif_playback::static_image(render.clone())));
        } else {
            // Why the page is not drawn, in the page's room, read as static
            // text: its failure, or until it arrives that it is prepared.
            body=body.child(message_line(format!("pdf-page-message-{index}"), error.unwrap_or_else(|| PREPARING.into()), Some(muted)));
        }
        if let Some(other) = &other.pdf && document.linked.load(Ordering::Relaxed) && other.count != document.count {
            body = body.child(div().mt_2().text_size(appearance::ui_text(11.)).child("The revisions have different page counts. Linked navigation stops each side at its own last page."));
        }
        // Why the last open opened nothing, as the header's last line.
        div().relative().flex_1().min_w_0().h_full().flex().flex_col().border_r_1().border_color(rgb(border)).child(crate::system_preview::header_ends_for_room(name, header, padding, held, vec![answer], end, body.into_any_element())).child(status)
            .on_prepaint(move |_,window,cx| { let _=view.update(cx,|_,cx|document.request(window,cx)); }).into_any_element()
    })).into_any_element()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use core::prelude::v1::test;

    /// Opens Go to page, the dialog a PDF's header opens, for a document of
    /// three pages no page of which is prepared.
    pub(crate) fn go_to_page(window: &mut Window, cx: &mut App) {
        let bytes: Arc<[u8]> = Arc::from(&b"%PDF-"[..]);
        let document = Document::new(bytes, 3, None, || Ok(())).expect("a document");
        page_picker(Arc::new(document), None, "Before", window, cx);
    }

    /// Waits until every page requested so far has answered; drawing then
    /// handles the answers.
    #[cfg(target_os = "linux")]
    pub(crate) fn drain_pages() {
        renderer().drain();
    }

    /// Links `document`'s pages to its partner's where they are independent,
    /// and otherwise makes them independent, as its link's click does on
    /// both sides.
    #[cfg(target_os = "linux")]
    pub(crate) fn flip_link(document: &Document) {
        document.linked.fetch_xor(true, Ordering::Relaxed);
    }

    /// `preview`, a changed PDF's comparison, as the page view draws it once
    /// a first page has been prepared: each side has three pages, Before's
    /// first prepared with its text, so it offers Read page text, and
    /// After's failed through the page lane, whose helper the tests cannot
    /// start ([`linux::helper_for`]; a build without the helper refuses), so
    /// it offers Retry page. Each side offers to link their pages.
    #[cfg(target_os = "linux")]
    pub(crate) fn paged(preview: &rich_preview::Comparison) -> crate::worker::Content {
        paged_at(preview, 3, 0)
    }

    /// [`paged`] with `count` pages a side, each side showing page `shown`
    /// (counted from 0), which Before has prepared only where it is the
    /// first; every other page fails as After's does. A page not prepared
    /// here is asked of the helper now, as the page lane would ask it, so
    /// the sides are drawn settled: a walk that records what each frame
    /// drew finds no page still being prepared, whose disabled Read page
    /// text the failure then replaces with Retry page.
    #[cfg(target_os = "linux")]
    pub(crate) fn paged_at(
        preview: &rich_preview::Comparison,
        count: usize,
        shown: usize,
    ) -> crate::worker::Content {
        let side = |side: &rich_preview::Side, first| {
            let bytes = side.captured.clone().expect("the side's bytes");
            let mut paged = rich_preview::Side::prepare(bytes.to_vec(), &side.name, || Ok(()))
                .expect("the side prepares");
            let document =
                Document::new(bytes.clone(), count, first, || Ok(())).expect("a document");
            {
                let mut state = document.state.lock().unwrap();
                state.page = shown;
                if state.cache.get(shown).is_none() {
                    match prepare_page(&bytes, shown, &|| Ok(())) {
                        Ok(prepared) => state.cache.insert(shown, prepared.page),
                        Err(error) => state.error = Some(format!("{error:#}")),
                    }
                }
            }
            paged.pdf = Some(Arc::new(document));
            Arc::new(paged)
        };
        let first = PreparedPage::new(
            page(),
            Ok(gitturtle_preview::PdfPageText {
                text: Some("Before's first page".into()),
                truncated: false,
            }),
        );
        crate::worker::Content::Rich(rich_preview::Comparison {
            old: side(&preview.old, Some(first)),
            new: side(&preview.new, None),
        })
    }

    fn page() -> rich_preview::Page {
        rich_preview::Page {
            render: None,
            width: 1,
            height: 1,
            caption: None,
            error: None,
        }
    }
    /// A page number's form is measured at its widest reading for the count,
    /// whatever page it shows, in a font whose digits differ in width, here
    /// one where "1" is narrower than the others, as in most proportional
    /// fonts (the tests' own font gives every character one width). Measured
    /// at the count alone, "Page 111 / 111", it was narrower than "Page 108
    /// / 111".
    #[test]
    fn a_page_number_is_measured_at_its_widest_page() {
        let width = |text: &str| -> u32 {
            text.chars()
                .map(|char| match char {
                    '1' => 4,
                    '0'..='9' => 7,
                    _ => 5,
                })
                .sum()
        };
        for count in [1, 3, 9, 10, 111, 1_000] {
            let [whole, short] = page_number_readings(count);
            let widest = |readings: &[String]| readings.iter().map(|r| width(r)).max().unwrap();
            for page in 1..=count {
                assert!(
                    width(&format!("Page {page} / {count}")) <= widest(&whole),
                    "Page {page} / {count} is no wider than its measure"
                );
                assert!(
                    width(&format!("{page}/{count}")) <= widest(&short),
                    "{page}/{count} is no wider than its measure"
                );
            }
        }
    }

    /// A page number's widest readings are laid out once for a count of so
    /// many digits at a font and size, and again for a count of more or
    /// fewer: kept for the font and size alone, a 111-page view would take
    /// the width of a three-page view's widest reading, "Page 9 / 9", and
    /// "Page 108 / 111" would be labelled where it does not fit, and cut.
    /// Built for every frame, the twenty readings were joined on each.
    #[gpui::test]
    fn a_page_number_is_measured_again_for_a_count_of_more_digits(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        cx.update(gpui_kit::init);
        let cx = cx.add_empty_window();
        // Every seed runs on one thread, which keeps the readings the last
        // laid out and the count of builds: each starts from no readings
        // and counts its own builds. Counted from the thread's first, the
        // second seed found five.
        BEFORE_PAGE_NUMBER.with(std::cell::RefCell::take);
        let before = PAGE_NUMBER_READINGS_BUILT.with(std::cell::Cell::get);
        let widths = cx.update(|window, _| {
            [3, 111, 107, 107, 3].map(|count| page_number_widths(0, count, window))
        });
        assert!(
            widths[1].0 > widths[0].0 && widths[1].1 > widths[0].1,
            "111 pages' widest readings {:?} are wider than 3 pages' {:?}",
            widths[1],
            widths[0]
        );
        assert_eq!(widths[2], widths[1], "107 pages' are 111 pages'");
        assert_eq!(widths[4], widths[0], "3 pages' are measured again");
        assert_eq!(
            PAGE_NUMBER_READINGS_BUILT.with(std::cell::Cell::get) - before,
            3,
            "built for 3 pages, for 111 and for 3 again; not for 107, whose digits are 111's"
        );
    }

    /// Each test thread prepares its pages on a lane of its own
    /// ([`renderer`]) and keeps it, so a test never meets a queue that tests
    /// running beside it filled. With one lane for the process, the 20-seed
    /// GPUI runs of this module failed two runs in six, each time a
    /// different test whose page was refused as busy.
    #[test]
    fn each_test_thread_has_its_own_page_lane() {
        let lane = || renderer() as *const SerialExecutor as usize;
        let here = lane();
        let there = std::thread::spawn(lane).join().unwrap();
        assert_ne!(here, there, "another thread has another lane");
        assert_eq!(here, lane(), "a thread keeps its lane");
    }

    #[test]
    fn cache_promotes_revisited_pages_and_evicts_oldest() {
        let mut cache = Cache::default();
        for index in 0..4 {
            cache.insert(index, page());
        }
        assert!(cache.get(0).is_some());
        cache.insert(4, page());
        assert!(cache.get(1).is_none());
        assert!(cache.get(0).is_some());
        assert_eq!(cache.pages.len(), 4);
        assert!(cache.bytes <= CACHE_BYTES);
    }
    #[test]
    fn text_and_pixels_share_the_pdf_cache_byte_budget() {
        let mut cache = Cache::default();
        for index in 0..4 {
            let image = gitturtle_preview::ImagePreview {
                width: 1000,
                height: 1000,
                original_width: 1000,
                original_height: 1000,
                rgba: vec![index as u8; 4_000_000],
                format: "fixture".into(),
            };
            let mut page = page();
            page.render = Some(worker::render_image(&image).unwrap());
            cache.insert(
                index,
                PreparedPage::new(
                    page,
                    Ok(gitturtle_preview::PdfPageText {
                        text: Some("a".repeat(gitturtle_preview::MAX_PDF_PAGE_TEXT_BYTES)),
                        truncated: true,
                    }),
                ),
            );
        }
        assert_eq!(cache.pages.len(), 3);
        assert!(cache.get(0).is_none());
        assert!(cache.get(3).is_some());
        assert!(cache.bytes <= CACHE_BYTES);
        assert!(Document::new(Arc::from([]), 1, None, || anyhow::bail!("cancelled")).is_err());
    }
    #[test]
    fn page_changes_reject_late_results_and_link_at_different_counts() {
        let before = Document::new(Arc::from([]), 20, None, || Ok(())).unwrap();
        let after = Document::new(Arc::from([]), 9, None, || Ok(())).unwrap();
        before.state.lock().unwrap().pending = true;
        before.linked.store(true, Ordering::Relaxed);
        before.select(15, Some(&after));
        assert_eq!(before.state.lock().unwrap().page, 15);
        assert_eq!(after.state.lock().unwrap().page, 8);
        assert!(!before.state.lock().unwrap().accepts(0, 0));
        before.pause();
        assert!(!before.state.lock().unwrap().pending);
    }

    /// Pages through GitTurtle's PDF helper on Linux, with shell fakes as the
    /// helper: each writes its pid to a file first, then plays its part.
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    pub(crate) mod linux {
        use super::*;
        use crate::pdf_helper::{Client, DEADLINE, HelperCommand, Slots};
        use gitturtle_preview::pdf_helper::PROTOCOL_VERSION;
        use std::{
            path::Path,
            time::{Duration, Instant},
        };

        /// A deadline longer than a test's drains wait (`operations::test_wait`,
        /// 30 s at the least): a helper still running when it should have
        /// been stopped holds its thread, and the drain fails rather than
        /// see the deadline stop it.
        const LONG: Duration = Duration::from_secs(60);

        /// A fake that never answers and ends with the test process: it
        /// waits while its parent, the test, lives. A failing run's fake
        /// then lasts until the client's deadline or the test's end, not
        /// beyond.
        const BLOCKS: &str = "while kill -0 $PPID 2>/dev/null; do sleep 0.05; done";

        /// The helpers tests named for documents' bytes ([`name_helper`]),
        /// the latest last.
        static NAMED: Mutex<Vec<(Vec<u8>, Arc<Client<'static>>)>> = Mutex::new(Vec::new());
        static UNSTARTABLE_SLOTS: Slots = Slots::new(2);

        /// The helper a test named for `bytes`, or else one that cannot
        /// start: a program that does not exist, so that no test runs this
        /// test binary as a helper and every page no test named fails at
        /// once with "The PDF renderer could not start."
        pub(crate) fn helper_for(bytes: &[u8]) -> Arc<Client<'static>> {
            if let Some((_, client)) = NAMED
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .rev()
                .find(|(named, _)| named == bytes)
            {
                return client.clone();
            }
            static UNSTARTABLE: std::sync::OnceLock<Arc<Client<'static>>> =
                std::sync::OnceLock::new();
            UNSTARTABLE
                .get_or_init(|| {
                    Arc::new(Client::with(
                        HelperCommand::new("/nonexistent/gitturtle-pdf-helper", [] as [&str; 0]),
                        DEADLINE,
                        &UNSTARTABLE_SLOTS,
                    ))
                })
                .clone()
        }

        /// From now on, `bytes`' pages come from a shell fake that writes
        /// its pid to `pid` and then runs `script`, stopped at `deadline`,
        /// with two helper slots of its own.
        fn name_helper(bytes: &[u8], pid: &Path, script: &str, deadline: Duration) {
            let script = format!(
                "PATH=/usr/bin:/bin; printf %s $$ > '{}'; {script}",
                pid.display()
            );
            let slots: &'static Slots = Box::leak(Box::new(Slots::new(2)));
            let client = Client::with(
                HelperCommand::new("/bin/sh", ["-c", script.as_str()]),
                deadline,
                slots,
            );
            NAMED
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((bytes.to_vec(), Arc::new(client)));
        }

        /// A script that prints `frame`, a reply, byte for byte.
        fn printing(frame: &[u8]) -> String {
            let octal: String = frame.iter().map(|byte| format!("\\{byte:03o}")).collect();
            format!("exec printf '{octal}'")
        }

        fn reply(status: u8) -> Vec<u8> {
            let mut frame = b"GTPr".to_vec();
            frame.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
            frame.push(status);
            frame
        }

        /// A 1 × 1 page reply for zero-based `page` of `count` pages: its
        /// text is `text`, or a renderer failure where `text` is `None`.
        fn page_frame(page: u32, count: u32, text: Option<&str>) -> Vec<u8> {
            let mut frame = reply(0);
            for value in [page, count, 1, 1, 1, 1] {
                frame.extend_from_slice(&value.to_le_bytes());
            }
            let words = text.unwrap_or_default();
            // Text present; or failed, with the renderer-failure kind (13).
            frame.extend_from_slice(if text.is_some() {
                &[1, 0, 0]
            } else {
                &[2, 13, 0]
            });
            for value in [words.len() as u32, 1, 2, 3] {
                frame.extend_from_slice(&value.to_le_bytes());
            }
            frame.extend_from_slice(&[9, 8, 7, 255]);
            frame.extend_from_slice(words.as_bytes());
            frame
        }

        /// An error reply of kind `code` with `detail`.
        fn error_frame(code: u8, detail: u32) -> Vec<u8> {
            let mut frame = reply(1);
            frame.push(code);
            frame.extend_from_slice(&detail.to_le_bytes());
            frame
        }

        /// The pid a fake wrote, once it has.
        fn pid_in(path: &Path) -> libc::pid_t {
            let started = Instant::now();
            loop {
                if let Some(pid) = std::fs::read_to_string(path)
                    .ok()
                    .and_then(|pid| pid.parse().ok())
                {
                    return pid;
                }
                assert!(
                    started.elapsed() < Duration::from_secs(10),
                    "the fake never started"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
        }

        /// The process is gone and reaped: not even a zombie is left.
        fn reaped(pid: libc::pid_t) -> bool {
            // SAFETY: signal 0 only checks that the process exists.
            let result = unsafe { libc::kill(pid, 0) };
            result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        }

        /// A small PDF whose bytes name the test, so that its helper is the
        /// one the test named.
        fn pdf(name: &str) -> Vec<u8> {
            format!("%PDF-1.4\n% {name}\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n")
                .into_bytes()
        }

        /// A comparison whose Before is a `count`-page document of `bytes`
        /// showing page `shown` (counted from 0), none of its pages
        /// prepared, and whose After is absent.
        fn before_only(
            bytes: &[u8],
            count: usize,
            shown: usize,
        ) -> (crate::worker::Content, Arc<Document>) {
            let document =
                Arc::new(Document::new(Arc::from(bytes), count, None, || Ok(())).unwrap());
            document.state.lock().unwrap().page = shown;
            let name = Path::new("document.pdf");
            let mut before = rich_preview::Side::unavailable(name, true, None);
            before.pdf = Some(document.clone());
            let content = crate::worker::Content::Rich(rich_preview::Comparison {
                old: Arc::new(before),
                new: Arc::new(rich_preview::Side::unavailable(name, false, None)),
            });
            (content, document)
        }

        struct Pages {
            content: crate::worker::Content,
            /// The comparison's width.
            width: Pixels,
            /// The comparison's height, 600 px unless a test makes it short
            /// enough for a header to scroll.
            height: Pixels,
            /// The comparison's showing, which lasts as long as the view,
            /// as Compare's lasts while it shows one file. Made for each
            /// frame, it made each side header's scrolling state again
            /// every frame, so a header that scrolls, both sides' at 18 pt
            /// in 80 px sides, never settled and asked for frame after frame.
            showing: crate::system_preview::Showing,
        }

        impl Render for Pages {
            fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                let crate::worker::Content::Rich(comparison) = &self.content else {
                    panic!("a document comparison");
                };
                let opener =
                    crate::system_preview::Opener::new(WeakEntity::new_invalid(), self.showing);
                // Drawn as Compare draws it, which draws a side without
                // pages, as a locked PDF's, on the generic document surface.
                div()
                    .w(self.width)
                    .h(self.height)
                    .flex()
                    .flex_col()
                    // Behind every text run a mark a test finds among the
                    // painted quads, as wide as the run drawn, its ellipsis
                    // included (`width_walk::text_mark`).
                    .text_bg(crate::width_walk::text_mark())
                    .child(rich_preview::render_comparison(
                        comparison, false, opener, self.width, window, cx,
                    ))
                    // The page number's dialog, drawn as the app's root view
                    // draws it.
                    .children(gpui_kit::component::Root::render_dialog_layer(window, cx))
            }
        }

        fn window(
            cx: &mut gpui_kit::TestAppContext,
            content: crate::worker::Content,
        ) -> (Entity<Pages>, &mut gpui_kit::VisualTestContext) {
            // A page answers on the render lane's thread, which wakes the
            // view's task from there, as the app's windows allow.
            cx.executor().allow_parking();
            cx.update(gpui_kit::init);
            // A drawn page's pixels are registered for release.
            cx.update(crate::image_lifetime::init);
            cx.update(|cx| appearance::ThemeChoice::Midnight.apply(None, cx));
            // The kit's Root first, as the app's windows have it, which holds
            // the page number's dialog.
            let pages = std::rc::Rc::new(std::cell::RefCell::new(None));
            let made = pages.clone();
            let (_, cx) = cx.add_window_view(move |window, cx| {
                let view = cx.new(|_| Pages {
                    content,
                    width: px(900.),
                    height: px(600.),
                    showing: crate::system_preview::Showing::next(),
                });
                *made.borrow_mut() = Some(view.clone());
                gpui_kit::component::Root::new(view, window, cx)
            });
            cx.simulate_resize(size(px(1000.), px(700.)));
            let view = pages.borrow_mut().take().expect("the view is made");
            (view, cx)
        }

        /// What the side says where its page would be drawn, read as static
        /// text, if it says anything.
        fn page_message(cx: &mut gpui_kit::VisualTestContext, id: &str) -> Option<String> {
            crate::native_accessibility::written::forget();
            crate::tags::tests::draw(cx);
            crate::native_accessibility::written::labels()
                .into_iter()
                .find(|(written, _)| written == id)
                .and_then(|(_, name)| name)
        }

        fn click(cx: &mut gpui_kit::VisualTestContext, selector: &'static str) {
            let bounds = cx
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} is drawn"));
            cx.simulate_click(bounds.center(), Modifiers::none());
        }

        /// A page that answers after the person moved to another page is
        /// dropped, and its helper is killed then rather than at its
        /// deadline: the page lane's check, which the page change fails,
        /// runs while the helper works. The next page is drawn with its
        /// text offered, and nothing of the late page is shown.
        #[gpui::test]
        fn a_late_page_after_a_page_change_is_dropped_and_its_helper_killed(
            cx: &mut gpui_kit::TestAppContext,
        ) {
            let fakes = tempfile::tempdir().unwrap();
            let bytes = pdf("a late page after a page change");
            // The first page's helper never answers.
            let first = fakes.path().join("first");
            name_helper(&bytes, &first, BLOCKS, LONG);
            let (content, document) = before_only(&bytes, 3, 0);
            let (_view, cx) = window(cx, content);
            crate::tags::tests::draw(cx);
            let first = pid_in(&first);
            // The second page's answers at once.
            let frame = page_frame(1, 3, Some("the second page"));
            name_helper(
                &bytes,
                &fakes.path().join("second"),
                &printing(&frame),
                LONG,
            );
            click(cx, "pdf-forward-0");
            // Asks for the second page behind the first page's request.
            crate::tags::tests::draw(cx);
            drain_pages();
            crate::tags::tests::draw(cx);
            assert!(
                reaped(first),
                "the first page's helper is killed and reaped once the page changes"
            );
            assert!(
                cx.debug_bounds("pdf-read-text-0").is_some(),
                "the second page arrived with its text, so it offers Read page text"
            );
            assert!(
                cx.debug_bounds("pdf-retry-0").is_none(),
                "the first page's late failure is dropped: nothing offers Retry page"
            );
            assert_eq!(page_message(cx, "pdf-page-message-0"), None);
            let mut state = document.state.lock().unwrap();
            assert_eq!((state.page, state.pending, &state.error), (1, false, &None));
            assert!(state.cache.get(0).is_none() && state.cache.get(1).is_some());
        }

        /// Back pauses the view's documents (`pdf_view::pause`, as a hidden
        /// tab and another file do): the helper at work is killed then, its
        /// late answer is dropped and leaves no error, and the page is asked
        /// for again when the view is drawn again.
        #[gpui::test]
        fn a_late_page_after_a_pause_is_dropped_and_its_helper_killed(
            cx: &mut gpui_kit::TestAppContext,
        ) {
            let fakes = tempfile::tempdir().unwrap();
            let bytes = pdf("a late page after Back");
            let slow = fakes.path().join("slow");
            name_helper(&bytes, &slow, BLOCKS, LONG);
            let (content, document) = before_only(&bytes, 3, 1);
            let (view, cx) = window(cx, content);
            crate::tags::tests::draw(cx);
            let slow = pid_in(&slow);
            view.update(cx, |view, _| pause(Some(&view.content)));
            drain_pages();
            // The late answer reaches the view's task, which drops it.
            cx.run_until_parked();
            assert!(
                reaped(slow),
                "the paused page's helper is killed and reaped"
            );
            {
                let mut state = document.state.lock().unwrap();
                assert_eq!((state.page, state.pending, &state.error), (1, false, &None));
                assert!(state.cache.get(1).is_none());
            }
            // Drawn again, as Compare is on its return, the page is asked for again.
            let frame = page_frame(1, 3, Some("the second page"));
            name_helper(&bytes, &fakes.path().join("again"), &printing(&frame), LONG);
            crate::tags::tests::draw(cx);
            drain_pages();
            crate::tags::tests::draw(cx);
            assert!(cx.debug_bounds("pdf-read-text-0").is_some());
            assert!(document.state.lock().unwrap().cache.get(1).is_some());
        }

        /// A `git fast-import` stream of one commit per document on `main`,
        /// oldest first, each writing it as `document.pdf`.
        fn commits(documents: &[&[u8]]) -> Vec<u8> {
            let mut input = Vec::new();
            for (index, document) in documents.iter().enumerate() {
                input.extend_from_slice(
                    format!(
                        "commit refs/heads/main\ncommitter Fixture <fixture@example.invalid> {} +0000\ndata 8\nrevision\nM 100644 inline document.pdf\ndata {}\n",
                        1_700_000_000 + index,
                        document.len()
                    )
                    .as_bytes(),
                );
                input.extend_from_slice(document);
                input.push(b'\n');
            }
            input.extend_from_slice(b"done\n");
            input
        }

        /// Back while a changed PDF's first page is still being prepared,
        /// on the repository worker before any page view exists, cancels
        /// that read: Before's helper is killed and reaped then rather than
        /// at its deadline, and After's helper never starts. Back once only
        /// paused the documents, which did not exist yet, and both helpers
        /// ran on to their deadlines in turn, each holding one of the app's
        /// two helper slots.
        #[gpui::test]
        async fn back_during_a_first_page_kills_its_helper_and_starts_no_other(
            cx: &mut gpui_kit::TestAppContext,
        ) {
            let fakes = tempfile::tempdir().unwrap();
            let before = pdf("Back during a first page: Before");
            let after = pdf("Back during a first page: After");
            let (started, other) = (fakes.path().join("before"), fakes.path().join("after"));
            name_helper(&before, &started, BLOCKS, LONG);
            name_helper(&after, &other, BLOCKS, LONG);
            let input = commits(&[&before, &after]);
            let (_fixture, app, cx) = crate::views::tests::history_window_from(cx, &input).await;
            cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
            let first = pid_in(&started);
            cx.update(|window, cx| app.update(cx, |app, cx| app.back_to_history(window, cx)));
            // The read ends once its check fails, which kills the helper.
            app.read_with(cx, |app, _| app.worker.wait_idle());
            assert!(
                reaped(first),
                "Before's helper is killed and reaped on Back"
            );
            assert!(!other.exists(), "After's helper never starts");
            // Nothing is left loading, and no comparison arrives.
            assert_eq!(app.read_with(cx, |app, _| app.loading), None);
            assert!(app.read_with(cx, |app, _| app.content.is_none()));
        }

        /// Every control of a side's header, by selector, with where it is
        /// drawn: the page's own control under whichever reading it shows.
        fn page_controls(
            cx: &mut gpui_kit::VisualTestContext,
        ) -> Vec<(&'static str, Bounds<Pixels>)> {
            // Each side's controls in the form it is drawn in: the compact
            // forms' menus hold the open control, the page's own control and
            // the link, and the narrowest form's the zooms too.
            let forms = DRAWN_FORMS.with_borrow(Clone::clone);
            let mut selectors = Vec::new();
            for side in 0..2 {
                let form = forms[&side];
                let names: &[&str] = match form {
                    HeaderForm::Full => &[
                        "pdf-system",
                        "pdf-back",
                        "pdf-forward",
                        "pdf-page-number",
                        "pdf-link",
                        "pdf-zoom-level",
                    ],
                    HeaderForm::Compact => &[
                        "pdf-back",
                        "pdf-forward",
                        "pdf-page-number",
                        "pdf-zoom-menu",
                        "pdf-more",
                    ],
                    HeaderForm::Narrowest => {
                        &["pdf-back", "pdf-forward", "pdf-page-number", "pdf-more"]
                    }
                };
                for name in names {
                    selectors.push(format!("{name}-{side}"));
                }
                if form == HeaderForm::Full {
                    selectors.extend((0..3).map(|zoom| format!("pdf-zoom-{}", side * 3 + zoom)));
                }
            }
            // The open control is drawn where the bytes establish a PDF; a
            // control that comes or goes between two readings fails their
            // comparison.
            let mut drawn: Vec<_> = selectors
                .into_iter()
                .filter_map(|selector| {
                    let selector: &'static str = selector.leak();
                    let bounds = cx.debug_bounds(selector);
                    assert!(
                        bounds.is_some() || selector.starts_with("pdf-system"),
                        "{selector} is drawn"
                    );
                    Some((selector, bounds?))
                })
                .collect();
            for side in 0..2 {
                if forms[&side] != HeaderForm::Full {
                    continue;
                }
                let shown = [format!("pdf-read-text-{side}"), format!("pdf-retry-{side}")]
                    .map(|selector| cx.debug_bounds(selector.leak()));
                let ([Some(bounds), None] | [None, Some(bounds)]) = shown else {
                    panic!("side {side} draws one page control: {shown:?}");
                };
                drawn.push((["pdf-page-control-0", "pdf-page-control-1"][side], bounds));
            }
            drawn
        }

        /// Nothing already drawn in a side's header moves while a page is
        /// prepared, in a linked comparison whose later pages are slow: Read
        /// page text keeps its place and width, disabled with its reason,
        /// until the page's text arrives; a failure brings Retry page in the
        /// same place; and Previous, Next, the page number, the link and the
        /// zooms keep their bounds throughout, before the page change, while
        /// the page prepares, once it fails and once a retry brings it, at
        /// 1480 × 973 and 461 × 490, at 13 and 18 pt. Read page text once
        /// left its place while the next page prepared, Next moved about
        /// 75 px left at 13 pt in 1480 × 973 and the link under the pointer,
        /// so a second click on Next unlinked the pages.
        #[gpui::test]
        async fn a_side_s_controls_keep_their_places_while_a_page_prepares(
            cx: &mut gpui_kit::TestAppContext,
        ) {
            let fakes = tempfile::tempdir().unwrap();
            let sides = [
                pdf("controls keep their places: Before"),
                pdf("controls keep their places: After"),
            ];
            // Each side's first page answers at once, with its text.
            for (index, bytes) in sides.iter().enumerate() {
                let first = printing(&page_frame(0, 3, Some("the first page")));
                name_helper(
                    bytes,
                    &fakes.path().join(format!("first-{index}")),
                    &first,
                    LONG,
                );
            }
            let input = commits(&[&sides[0], &sides[1]]);
            // A page answers on the render lane's thread, which wakes the
            // view's task from there.
            cx.executor().allow_parking();
            let (_fixture, app, cx) = crate::views::tests::history_window_from(cx, &input).await;
            cx.update(|_, cx| appearance::ThemeChoice::Midnight.apply(None, cx));
            cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
            app.read_with(cx, |app, _| app.worker.wait_idle());
            crate::views::tests::settle(&app, cx).await;
            let documents = app.read_with(cx, |app, _| {
                let Some(crate::worker::Content::Rich(comparison)) = app.content.as_deref() else {
                    panic!("a changed PDF is a document comparison");
                };
                [&comparison.old, &comparison.new]
                    .map(|side| side.pdf.clone().expect("each side has its pages"))
            });
            // Linked, as a click on either side's link does.
            for document in &documents {
                flip_link(document);
            }
            let redraw = |cx: &mut gpui_kit::VisualTestContext| {
                app.update(cx, |_, cx| cx.notify());
                crate::views::tests::draw(cx);
            };
            // Whether side `side` offers its page's own control reading
            // `reading`: drawn in the full header, and in the compact forms
            // held by the "more" menu, whose tooltip lists what it holds
            // after its reading.
            let offers = |cx: &mut gpui_kit::VisualTestContext, side: usize, reading: &str| {
                let selector = match reading {
                    "Retry page" => format!("pdf-retry-{side}"),
                    _ => format!("pdf-read-text-{side}"),
                };
                let more = format!("pdf-more-{side}");
                cx.debug_bounds(selector.leak()).is_some()
                    || (cx.debug_bounds(more.clone().leak()).is_some()
                        && crate::system_preview::DRAWN_CONTROLS.with_borrow(|drawn| {
                            drawn.get(&more).is_some_and(|more| {
                                more.tooltip.as_deref().is_some_and(|tip| {
                                    tip.rsplit_once(" · ").is_some_and(|(_, held)| {
                                        held.split(", ").any(|item| item == reading)
                                    })
                                })
                            })
                        }))
            };
            for (width, height) in [(1480., 973.), (461., 490.)] {
                for text in [13, 18] {
                    let at = format!("{width} × {height} at {text} pt");
                    cx.simulate_resize(size(px(width), px(height)));
                    cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
                    // Both sides on their first page, the second not yet
                    // prepared.
                    documents[0].select(0, Some(&documents[1]));
                    for document in &documents {
                        let mut state = document.state.lock().unwrap();
                        state.cache.pages.retain(|(page, _, _)| *page == 0);
                        state.cache.bytes = state.cache.pages.iter().map(|page| page.2).sum();
                    }
                    redraw(cx);
                    let before = page_controls(cx);
                    assert!(
                        offers(cx, 0, "Read page text"),
                        "{at}: the first page offers Read page text"
                    );
                    // Next on Before, which moves both linked sides, each
                    // of whose second page is then being prepared: its
                    // request is pending, as the page lane holds it while
                    // a slow helper works. The states are set here rather
                    // than by helpers that block the page lane: once one
                    // lane for the whole test process, held while bounds
                    // were read, it refused other tests' pages as busy.
                    documents[0].select(1, Some(&documents[1]));
                    for document in &documents {
                        document.state.lock().unwrap().pending = true;
                    }
                    redraw(cx);
                    assert_eq!(
                        page_controls(cx),
                        before,
                        "{at}: nothing in either header moves while the page prepares"
                    );
                    // Each side's page fails, as a stopped helper's does.
                    for document in &documents {
                        let mut state = document.state.lock().unwrap();
                        state.pending = false;
                        state.error = Some("The PDF renderer stopped on this page.".into());
                    }
                    redraw(cx);
                    assert!(
                        offers(cx, 0, "Retry page") && offers(cx, 1, "Retry page"),
                        "{at}: each failed page offers Retry page"
                    );
                    assert_eq!(
                        page_controls(cx),
                        before,
                        "{at}: nothing in either header moves when the page fails"
                    );
                    // A retry brings each page with its text.
                    for document in &documents {
                        let mut state = document.state.lock().unwrap();
                        state.error = None;
                        let text = gitturtle_preview::PdfPageText {
                            text: Some("the second page".into()),
                            truncated: false,
                        };
                        state.cache.insert(1, PreparedPage::new(page(), Ok(text)));
                    }
                    redraw(cx);
                    assert!(
                        offers(cx, 0, "Read page text") && offers(cx, 1, "Read page text"),
                        "{at}: each side's second page offers Read page text"
                    );
                    assert_eq!(
                        page_controls(cx),
                        before,
                        "{at}: nothing in either header moves when the page arrives"
                    );
                }
            }
        }

        /// A page stopped at its deadline says so in the page's room and
        /// offers Retry page, not Read page text; Retry page asks for that
        /// page again (the fake's answer is for that page alone), and the
        /// page then arrives with its text.
        #[gpui::test]
        fn a_page_past_its_deadline_says_so_and_retry_page_asks_for_it_again(
            cx: &mut gpui_kit::TestAppContext,
        ) {
            let fakes = tempfile::tempdir().unwrap();
            let bytes = pdf("a page past its deadline");
            // A deadline of zero has passed as soon as the helper starts.
            let stopped = fakes.path().join("stopped");
            name_helper(&bytes, &stopped, BLOCKS, Duration::ZERO);
            let (content, document) = before_only(&bytes, 3, 1);
            let (_view, cx) = window(cx, content);
            crate::tags::tests::draw(cx);
            drain_pages();
            assert_eq!(
                page_message(cx, "pdf-page-message-0").as_deref(),
                Some("The PDF page took longer than 10 seconds and was stopped.")
            );
            assert!(cx.debug_bounds("pdf-retry-0").is_some());
            assert!(
                cx.debug_bounds("pdf-read-text-0").is_none(),
                "no text arrived for the stopped page"
            );
            let frame = page_frame(1, 3, Some("the second page"));
            name_helper(
                &bytes,
                &fakes.path().join("retried"),
                &printing(&frame),
                LONG,
            );
            click(cx, "pdf-retry-0");
            crate::tags::tests::draw(cx);
            drain_pages();
            crate::tags::tests::draw(cx);
            assert!(cx.debug_bounds("pdf-retry-0").is_none());
            assert!(cx.debug_bounds("pdf-read-text-0").is_some());
            assert_eq!(page_message(cx, "pdf-page-message-0"), None);
            let mut state = document.state.lock().unwrap();
            let page = state.cache.get(1).expect("the retried page");
            assert_eq!(page.text.as_deref(), Some("the second page"));
        }

        /// A helper that cannot confine itself, as on a kernel without
        /// seccomp, exits with its confinement code: the first page's side
        /// says the renderer could not start and offers no Retry page, as a
        /// failed first page offers none on macOS, and a later page says
        /// the same in its room with Retry page.
        #[gpui::test]
        fn a_renderer_that_could_not_start_says_so(cx: &mut gpui_kit::TestAppContext) {
            let fakes = tempfile::tempdir().unwrap();
            let bytes = pdf("a helper without seccomp");
            let script = format!("exit {}", crate::pdf_helper::exit::CONFINEMENT);
            name_helper(&bytes, &fakes.path().join("pid"), &script, LONG);
            let first =
                rich_preview::Side::prepare(bytes.clone(), Path::new("document.pdf"), || Ok(()))
                    .unwrap();
            assert!(first.pdf.is_none());
            let (later, _) = before_only(&bytes, 3, 1);
            let content = crate::worker::Content::Rich(rich_preview::Comparison {
                old: Arc::new(first),
                new: match &later {
                    crate::worker::Content::Rich(later) => later.old.clone(),
                    _ => unreachable!(),
                },
            });
            let (_view, cx) = window(cx, content);
            crate::tags::tests::draw(cx);
            drain_pages();
            let sentence = Some("The PDF renderer could not start.".to_owned());
            assert_eq!(page_message(cx, "pdf-side-message-0"), sentence);
            assert_eq!(page_message(cx, "pdf-page-message-1"), sentence);
            assert!(cx.debug_bounds("pdf-retry-0").is_none());
            assert!(cx.debug_bounds("pdf-retry-1").is_some());
        }

        /// Fit, −, + and the zoom are one run: wherever a line of the header
        /// holds them, they lie on one line, moving to the next whole rather
        /// than break, at every side width from 150 to 500 px at 13 and
        /// 18 pt. A side with no link, as an added PDF's, once ended its
        /// first line with Fit at 1480 × 973 and began the next with − +
        /// 100%.
        #[gpui::test]
        fn the_zooms_are_one_run_that_moves_whole(cx: &mut gpui_kit::TestAppContext) {
            let (content, document) = before_only(&pdf("the zooms are one run"), 3, 0);
            // Drawn with its pages prepared, so the page lane is never
            // asked for one.
            document
                .state
                .lock()
                .unwrap()
                .cache
                .insert(0, PreparedPage::from(page()));
            let (view, cx) = window(cx, content);
            let mut moved = false;
            for text in [13, 18] {
                cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
                for width in (300..=1000).step_by(5) {
                    view.update(cx, |view, cx| {
                        view.width = px(width as f32);
                        cx.notify();
                    });
                    crate::tags::tests::draw(cx);
                    // The zooms are a run in the full form alone: the compact
                    // forms fold them into a menu.
                    if DRAWN_FORMS.with_borrow(|drawn| drawn[&0]) != HeaderForm::Full {
                        continue;
                    }
                    let mut bounds = |selector: &'static str| {
                        cx.debug_bounds(selector)
                            .unwrap_or_else(|| panic!("{selector} is drawn"))
                    };
                    let run = ["pdf-zoom-1", "pdf-zoom-0", "pdf-zoom-2", "pdf-zoom-level-0"]
                        .map(&mut bounds);
                    let (header, back) = (bounds("pdf-side-header-0"), bounds("pdf-back-0"));
                    // The header's `p_2` and `gap_2`.
                    let half = cx.update(|window, _| window.rem_size()) * 0.5;
                    let natural = run
                        .iter()
                        .fold(half * 3., |width, drawn| width + drawn.size.width);
                    let at = format!("{text} pt, side {} px: {run:?}", width / 2);
                    if natural <= header.size.width - half * 2. {
                        let centre = run[0].center().y;
                        assert!(
                            run.iter()
                                .all(|drawn| (drawn.center().y - centre).abs() < px(0.5))
                                && run.windows(2).all(|pair| pair[0].right() < pair[1].left()),
                            "{at}: the zooms lie on one line, in order"
                        );
                    }
                    moved |= run[0].top() > back.bottom()
                        && (run[0].left() - header.left() - half).abs() < px(0.5);
                }
            }
            assert!(moved, "at some width the run starts a line of its own");
        }

        /// A side's heading gives way rather than be cut, a step at a time,
        /// at every side width from 50 to 400 px at 13 and 18 pt: where a
        /// line of the header holds "Before · PDF" it reads whole; where it
        /// holds only "Before", it reads the side's name alone, as wide as
        /// "Before" is laid out; and where not even that fits, it reads the
        /// name, ending in an ellipsis at its line's end. It always lies
        /// inside its header and ends at that line's end, short of the
        /// gutter the header's scrollbar takes where the header scrolls,
        /// even in the first frame it scrolls in, as a box shortens, before
        /// its region has seen it overflow and while its lines are still as
        /// wide as the region, where the heading's cap alone holds it: the
        /// test harness draws that frame before the test's own draw, so the
        /// heading is read in every frame painted (`DRAWN_HEADINGS`). As
        /// wide as its header, the heading was drawn whole into that gutter
        /// at 13 pt in 58 to 63 px sides; uncapped, "Before" was drawn whole
        /// in that first frame, 47 px in 28 to 46 px lines at 13 pt. At
        /// 18 pt in a 461 × 490 window it was laid out whole in a 121 px
        /// side and cut at the side's edge, "Before · PD"; a heading kept
        /// whole where a line holds only "Before" is wider than "Before",
        /// whole or cut, and fails the middle step.
        #[gpui::test]
        fn a_side_s_heading_gives_way_rather_than_be_cut(cx: &mut gpui_kit::TestAppContext) {
            let (content, document) = before_only(&pdf("a side's heading"), 3, 0);
            // Drawn with its pages prepared, so the page lane is never
            // asked for one.
            document
                .state
                .lock()
                .unwrap()
                .cache
                .insert(0, PreparedPage::from(page()));
            let (view, cx) = window(cx, content);
            let mut reached = std::collections::BTreeSet::new();
            for text in [13, 18] {
                cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
                // Each reading as the heading's text lays it out, semibold.
                let (whole, name) = cx.update(|window, _| {
                    let size = window.text_style().font_size.to_pixels(window.rem_size());
                    let width = |text: &str| {
                        crate::views::shaped_width(text, size, FontWeight::SEMIBOLD, window).ceil()
                    };
                    (width("Before · PDF"), width("Before"))
                });
                let mut holding = rich_preview::Side::unavailable(Path::new("a.pdf"), true, None);
                holding.pdf = Some(document.clone());
                let heading = cx.update(|window, _| {
                    side_heading(0, "Before", &holding, px(1000.), true, window)
                });
                assert_eq!(
                    (heading.text.as_str(), heading.caption, heading.width),
                    ("Before · PDF", None, whole)
                );
                // Narrowing, as a window does, so each step follows the one
                // before it.
                for width in (100..=800).rev().step_by(2) {
                    view.update(cx, |view, cx| {
                        view.width = px(width as f32);
                        cx.notify();
                    });
                    crate::tags::tests::draw(cx);
                    let header = cx.debug_bounds("pdf-side-header-0").expect("a header");
                    let heading = cx.debug_bounds("pdf-side-heading-0").expect("a heading");
                    // The line the header was laid out on, as layout gives
                    // the narrower side ([`pair_line`]): less the gutter its
                    // scrollbar takes only where it scrolls.
                    let scrolls = DRAWN_LAYOUTS.with_borrow(|drawn| drawn[&0].scrolls);
                    let (half, line) = cx.update(|window, _| {
                        (
                            window.rem_size() * 0.5,
                            pair_line(px(width as f32), false, scrolls, window),
                        )
                    });
                    let at = format!(
                        "{text} pt, side {} px, line {line:?}: {heading:?} in {header:?}",
                        width / 2
                    );
                    assert!(
                        heading.left() >= header.left() + half - px(0.5)
                            && heading.right() <= header.right() - half + px(0.5),
                        "{at}: the heading lies inside its header"
                    );
                    // And inside the line's end, the side less its right
                    // rule, the header's inset and, where the header
                    // scrolls, the gutter its scrollbar takes, found from the
                    // side's width rather than the header's: the heading, as
                    // wide as the header, was drawn whole into that gutter at
                    // 13 pt in 58 to 63 px sides.
                    assert!(
                        heading.right() <= header.left() + line + half + px(0.5),
                        "{at}: the heading ends at its line's end"
                    );
                    let step = if whole <= line {
                        assert!(
                            (heading.size.width - whole).abs() <= px(1.),
                            "{at}: where a line holds \"Before · PDF\" the heading reads it whole"
                        );
                        "whole"
                    } else if name <= line {
                        assert!(
                            (heading.size.width - name).abs() <= px(1.),
                            "{at}: where a line holds only \"Before\" the heading reads it, as wide as {name:?}"
                        );
                        "name"
                    } else {
                        // Cut at the line's end, short of the gutter.
                        assert!(
                            (heading.size.width - line).abs() <= px(1.),
                            "{at}: where a line does not hold \"Before\" the heading reads it, cut at its line's end"
                        );
                        "ellipsis"
                    };
                    reached.insert((text, step));
                }
                // The first frame a side's header scrolls in, as a box
                // shortens: its lines leave the scrollbar's gutter in that
                // frame, while its region still lays them out as wide as it
                // did before it overflowed (`tags::ScrollCue` follows the
                // frame before), so the heading is held to its line there
                // by its cap alone. In the sides whose line holds "Before"
                // without the gutter but not with it, "Before" was drawn
                // whole into the gutter for that frame. The test harness
                // draws that frame itself, as the change makes the window
                // dirty, before the test's own draw, which sees the gutter
                // applied: the heading is read in every frame painted
                // (`DRAWN_HEADINGS`), the first and the last.
                let half = cx.update(|window, _| window.rem_size() * 0.5);
                let least = f32::from(name + px(1.) + half * 2.).ceil() as usize;
                let gutter = f32::from(crate::tags::ScrollCue::gutter()) as usize;
                eprintln!(
                    "{text} pt: first frames at sides {least} to {} px",
                    least + gutter - 1
                );
                for side in least..least + gutter {
                    view.update(cx, |view, cx| {
                        (view.width, view.height) = (px(side as f32 * 2.), px(600.));
                        cx.notify();
                    });
                    crate::tags::tests::draw(cx);
                    DRAWN_HEADINGS.set(Some(Vec::new()));
                    view.update(cx, |view, cx| {
                        view.height = px(140.);
                        cx.notify();
                    });
                    cx.update(|window, cx| {
                        window.simulate_next_frame(cx);
                        window.draw(cx).clear(cx);
                    });
                    let painted: Vec<Bounds<Pixels>> = DRAWN_HEADINGS
                        .take()
                        .into_iter()
                        .flatten()
                        .filter_map(|(index, heading)| (index == 0).then_some(heading))
                        .collect();
                    let header = cx.debug_bounds("pdf-side-header-0").expect("a header");
                    let line = crate::system_preview::side_line_room(px(side as f32), half, true);
                    let at = format!(
                        "{text} pt, side {side} px, in 140 px, line {line:?}, header {header:?}"
                    );
                    assert!(
                        DRAWN_LAYOUTS.with_borrow(|drawn| drawn[&0].scrolls),
                        "{at}: the header scrolls"
                    );
                    assert!(
                        painted.len() >= 2,
                        "{at}: the change and the test each paint a frame: {painted:?}"
                    );
                    for (frame, heading) in painted.iter().enumerate() {
                        assert!(
                            heading.right() <= header.left() + line + half + px(0.5),
                            "{at}: in frame {frame} of {} the heading {heading:?} ends at its line's end, short of the gutter",
                            painted.len()
                        );
                    }
                }
                view.update(cx, |view, cx| {
                    view.height = px(600.);
                    cx.notify();
                });
            }
            for text in [13, 18] {
                for step in ["whole", "name", "ellipsis"] {
                    assert!(
                        reached.contains(&(text, step)),
                        "at {text} pt the heading reaches its {step} step"
                    );
                }
            }
        }

        /// `document` shows page `number` (counted from 0), prepared, as it
        /// is once Next or the page field brings it and it arrives, so the
        /// page lane is never asked for one.
        fn show(document: &Document, number: usize) {
            document.select(number, None);
            let mut state = document.state.lock().unwrap();
            if state.cache.get(number).is_none() {
                state.cache.insert(number, PreparedPage::from(page()));
            }
        }

        /// A comparison of two `count`-page documents, each side showing its
        /// first page prepared ([`show`]), offering to link its pages,
        /// independent, and offering its captured bytes to another app as
        /// the app does on this platform ([`sides_of`]).
        fn both_sides(name: &str, count: usize) -> (crate::worker::Content, [Arc<Document>; 2]) {
            sides_of(name, [count, count])
        }

        /// [`both_sides`] of `counts` pages, Before's and After's, each side
        /// offering its captured bytes to another app where the app does on
        /// this platform, as it does for a PDF on Linux and macOS
        /// ([`sides_offering`]), so the full form's first line holds the
        /// open control as the app's does. Without it the tests' full form
        /// began at a narrower side than with it, where the app's begins: at
        /// 13 pt and scale 1.2, 307.5 px for 342.5 px in the tests' font
        /// ([`a_narrow_side_takes_a_compact_header_that_never_scrolls`]),
        /// and natively 314 px.
        fn sides_of(
            name: &str,
            counts: [usize; 2],
        ) -> (crate::worker::Content, [Arc<Document>; 2]) {
            sides_offering(name, counts, true)
        }

        /// [`sides_of`], each side offering its captured bytes to another
        /// app where `open` and the platform does for a PDF
        /// (`system_preview::offer`), as the app's worker decides it.
        fn sides_offering(
            name: &str,
            counts: [usize; 2],
            open: bool,
        ) -> (crate::worker::Content, [Arc<Document>; 2]) {
            let bytes = [0, 1].map(|side| pdf(&format!("{name}: side {side}")));
            let documents = [0, 1].map(|side| {
                let document = Document::new(
                    Arc::from(bytes[side].clone()),
                    counts[side],
                    None,
                    || Ok(()),
                )
                .unwrap();
                show(&document, 0);
                Arc::new(document)
            });
            let name = Path::new("document.pdf");
            let [old, new] = [0, 1].map(|at| {
                let mut side = rich_preview::Side::unavailable(name, true, None);
                side.pdf = Some(documents[at].clone());
                if open {
                    let bytes: Arc<[u8]> = Arc::from(bytes[at].clone());
                    side.system_preview =
                        crate::system_preview::offer(&bytes, &|| Ok(())).expect("an offer");
                    side.captured = Some(bytes);
                }
                Arc::new(side)
            });
            let content = crate::worker::Content::Rich(rich_preview::Comparison { old, new });
            (content, documents)
        }

        /// What follows each side's page number: the link and the zooms in
        /// the full form, the zoom menu and the "more" menu in the compact,
        /// and the "more" menu alone in the narrowest.
        const AFTER_THE_NUMBER: [&str; 8] = [
            "pdf-link-0",
            "pdf-zooms-0",
            "pdf-zoom-menu-0",
            "pdf-more-0",
            "pdf-link-1",
            "pdf-zooms-1",
            "pdf-zoom-menu-1",
            "pdf-more-1",
        ];

        /// Checks that what follows each side's page number
        /// ([`AFTER_THE_NUMBER`]) lies where it lay in the first of `states`
        /// in the second as well, at every comparison width from 160 to
        /// 1,000 px at 13 and 18 pt, and returns the forms `control` was
        /// drawn in, by text size.
        fn walk_widths(
            cx: &mut gpui_kit::VisualTestContext,
            view: &Entity<Pages>,
            control: &str,
            states: [&dyn Fn(); 2],
        ) -> std::collections::BTreeSet<(u8, &'static str)> {
            let mut forms = std::collections::BTreeSet::new();
            for text in [13u8, 18] {
                cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
                for width in (160..=1000).step_by(8) {
                    view.update(cx, |view, _| view.width = px(width as f32));
                    let drawn = states.map(|state| {
                        state();
                        crate::system_preview::DRAWN_CONTROLS
                            .with_borrow_mut(|drawn| drawn.clear());
                        view.update(cx, |_, cx| cx.notify());
                        crate::tags::tests::draw(cx);
                        if let Some(form) = crate::system_preview::DRAWN_CONTROLS
                            .with_borrow(|drawn| drawn.get(control).map(|drawn| drawn.form))
                        {
                            forms.insert((text, form));
                        }
                        let forms = DRAWN_FORMS.with_borrow(Clone::clone);
                        (
                            forms,
                            AFTER_THE_NUMBER.map(|selector| cx.debug_bounds(selector)),
                        )
                    });
                    // What each side's form draws after its page number: the
                    // link and the zooms, the zoom menu and the "more" menu,
                    // or the "more" menu alone.
                    let [(forms, _), _] = &drawn;
                    for side in 0..2 {
                        let after: &[&str] = match forms[&side] {
                            HeaderForm::Full => &["pdf-link", "pdf-zooms"],
                            HeaderForm::Compact => &["pdf-zoom-menu", "pdf-more"],
                            HeaderForm::Narrowest => &["pdf-more"],
                        };
                        for name in after {
                            let selector = format!("{name}-{side}");
                            let at = AFTER_THE_NUMBER
                                .iter()
                                .position(|drawn| *drawn == selector)
                                .expect("a control after the number");
                            assert!(
                                drawn[0].1[at].is_some(),
                                "{text} pt, side {} px: {selector} is drawn",
                                width / 2
                            );
                        }
                    }
                    assert_eq!(
                        drawn[0],
                        drawn[1],
                        "{text} pt, side {} px: the forms, and the controls after the number, keep their places",
                        width / 2
                    );
                }
            }
            forms
        }

        /// A page turn moves nothing after the page number, which takes the
        /// width of its widest reading in the form it is drawn in: each
        /// side's link and zooms keep their places on page 1 and page 10 of
        /// 10, whose readings differ most in the tests' font (one width for
        /// every character), labelled "Page 1 / 10" and short "1/10", at
        /// every side width from 80 to 500 px at 13 and 18 pt. Drawn at its
        /// own reading's width, the page number grew with "Page 10 / 10",
        /// and in a proportional font with any page whose digits are wider,
        /// so the link and the zooms moved on a page turn.
        #[gpui::test]
        fn a_page_turn_moves_nothing_after_the_page_number(cx: &mut gpui_kit::TestAppContext) {
            let (content, documents) = both_sides("a page turn", 10);
            let (view, cx) = window(cx, content);
            let turn = |number| {
                for document in &documents {
                    show(document, number);
                }
            };
            let forms = walk_widths(cx, &view, "pdf-page-number-0", [&|| turn(0), &|| turn(9)]);
            for text in [13, 18] {
                for form in ["labelled", "short"] {
                    assert!(
                        forms.contains(&(text, form)),
                        "at {text} pt the page number is drawn {form}"
                    );
                }
            }
        }

        /// Linking the pages moves nothing after the link, which takes the
        /// width of its wider reading where it is labelled: each side's link
        /// and zooms keep their places with the pages independent and
        /// linked, at every side width from 80 to 500 px at 13 and 18 pt.
        /// Drawn at its own reading's width, "Linked pages" was narrower
        /// than "Independent pages", so each click on the link moved the
        /// zooms beside it.
        #[gpui::test]
        fn linking_the_pages_moves_nothing_after_the_link(cx: &mut gpui_kit::TestAppContext) {
            let (content, documents) = both_sides("linking the pages", 3);
            let (view, cx) = window(cx, content);
            let link = |linked| {
                for document in &documents {
                    document.linked.store(linked, Ordering::Relaxed);
                }
            };
            let forms = walk_widths(cx, &view, "pdf-link-0", [&|| link(false), &|| link(true)]);
            // Labelled only: a side too narrow for it takes the compact
            // header, whose "more" menu holds it.
            for text in [13, 18] {
                assert!(
                    forms.contains(&(text, "labelled")),
                    "at {text} pt the link is drawn labelled"
                );
            }
        }

        /// The selectors of side 0's header controls in `form`, in order.
        fn header_controls(form: HeaderForm) -> &'static [&'static str] {
            match form {
                HeaderForm::Full => &[
                    "pdf-read-text-0",
                    "pdf-back-0",
                    "pdf-forward-0",
                    "pdf-page-number-0",
                    "pdf-link-0",
                    "pdf-zoom-1",
                    "pdf-zoom-0",
                    "pdf-zoom-2",
                    "pdf-zoom-level-0",
                ],
                HeaderForm::Compact => &[
                    "pdf-back-0",
                    "pdf-page-number-0",
                    "pdf-forward-0",
                    "pdf-zoom-menu-0",
                    "pdf-more-0",
                ],
                HeaderForm::Narrowest => &[
                    "pdf-back-0",
                    "pdf-page-number-0",
                    "pdf-forward-0",
                    "pdf-more-0",
                ],
            }
        }

        /// A side too narrow for its whole header takes a compact form
        /// rather than a region that scrolls, and a side too narrow for that
        /// the narrowest, each chosen from the side's drawn width, text size
        /// and scale alone (in a box tall enough for every form). At 13 and
        /// 18 pt and scales 1, 1.2 and 2, the narrowest full side and the
        /// narrowest compact side are found by bisection on device pixels
        /// (from 40 to 500 px), and at each and one device pixel narrower
        /// each side's header: takes the form expected, the full, compact or
        /// narrowest, the same in a second frame, as Before's page turns from
        /// 9 to 10 of 10 and while it is prepared; draws every control of its
        /// form, Previous, the page number, Next and, compact, the zoom menu
        /// and the "more" menu, or narrowest, the "more" menu alone, inside
        /// its header, which ends inside its region, keeps no scrollbar's
        /// gutter and gives that region no scroll; and keeps every control
        /// where it was through the turn and the preparing. At 18 pt in a
        /// 461 × 490 window a header that scrolled kept its controls out of
        /// reach of assistive technology until scrolled, and cut its
        /// messages. Each side offers its bytes to another app, as the app
        /// does for a PDF here ([`sides_of`]), so the full form's first line
        /// holds the open control. In the tests' font, from 2026-11-03, the
        /// compact form begins at a side of 244, 250 and 247 px at 13 pt
        /// (scales 1, 1.2 and 2) and 343, 345.83 and 342.5 px at 18 pt, and
        /// the full form at 338, 342.5 and 340 px at 13 pt and 471, 473.33
        /// and 471 px at 18 pt; with no open control the full form began at
        /// 303, 307.5 and 305.5 px at 13 pt and 423, 425 and 422.5 px at
        /// 18 pt, where natively, at 13 pt and scale 1.2, it began at 314 px.
        #[gpui::test]
        fn a_narrow_side_takes_a_compact_header_that_never_scrolls(
            cx: &mut gpui_kit::TestAppContext,
        ) {
            let started = std::time::Instant::now();
            let (content, documents) = both_sides("a compact header", 10);
            let (view, cx) = window(cx, content);
            let id = view.entity_id();
            let form_at = |cx: &mut gpui_kit::VisualTestContext, side: f32| {
                view.update(cx, |view, cx| {
                    view.width = px(side * 2.);
                    cx.notify();
                });
                crate::tags::tests::draw(cx);
                DRAWN_FORMS.with_borrow(|drawn| drawn[&0])
            };
            // The narrowest side, in device pixels, in (`narrow`, `wide`]
            // whose form is `form` or a wider one.
            let bisect = |cx: &mut gpui_kit::VisualTestContext,
                          (mut narrow, mut wide): (i32, i32),
                          form: HeaderForm,
                          scale: f32| {
                while wide - narrow > 1 {
                    let middle = (narrow + wide) / 2;
                    if form_at(cx, middle as f32 / scale) <= form {
                        wide = middle;
                    } else {
                        narrow = middle;
                    }
                }
                (narrow, wide)
            };
            let mut boundaries = Vec::new();
            for scale in [1f32, 1.2, 2.] {
                cx.update(|window, _| window.set_scale_factor(scale));
                for text in [13u8, 18] {
                    cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
                    let (least, most) = ((40. * scale) as i32, (500. * scale) as i32);
                    assert_eq!(form_at(cx, least as f32 / scale), HeaderForm::Narrowest);
                    assert_eq!(form_at(cx, most as f32 / scale), HeaderForm::Full);
                    let full = bisect(cx, (least, most), HeaderForm::Full, scale);
                    let compact = bisect(cx, (least, full.0), HeaderForm::Compact, scale);
                    boundaries.push((scale, text, compact.1 as f32 / scale, full.1 as f32 / scale));
                    for (device, form) in [
                        (compact.0, HeaderForm::Narrowest),
                        (compact.1, HeaderForm::Compact),
                        (full.0, HeaderForm::Compact),
                        (full.1, HeaderForm::Full),
                    ] {
                        let side = device as f32 / scale;
                        let at = format!("scale {scale}, {text} pt, side {side} px");
                        assert_eq!(form_at(cx, side), form, "{at}: the form");
                        // Page 9 of 10, then 10 of 10, then 10 prepared.
                        let states: [&dyn Fn(); 3] = [
                            &|| show(&documents[0], 8),
                            &|| show(&documents[0], 9),
                            &|| {
                                let mut state = documents[0].state.lock().unwrap();
                                state.cache = Cache::default();
                                state.pending = true;
                            },
                        ];
                        let mut placed = None;
                        for (step, state) in states.iter().enumerate() {
                            state();
                            for frame in 0..2 {
                                view.update(cx, |_, cx| cx.notify());
                                crate::tags::tests::draw(cx);
                                let layout = DRAWN_LAYOUTS.with_borrow(|drawn| drawn[&0]);
                                assert!(
                                    layout.form == form && !layout.scrolls,
                                    "{at}: step {step}, frame {frame} keeps the form, with no gutter: {layout:?}"
                                );
                            }
                            let header = cx.debug_bounds("pdf-side-header-0").expect("a header");
                            let region = cx.debug_bounds("pdf-side-0/header").expect("a region");
                            let scroll = cx
                                .update(|_, cx| {
                                    crate::system_preview::side_header(id, "pdf-side-0", cx)
                                })
                                .expect("the side's header")
                                .scroll()
                                .max_offset();
                            assert!(
                                scroll.y == px(0.) && header.bottom() <= region.bottom() + px(0.01),
                                "{at}: step {step}: the header {header:?} ends in its region {region:?}, which does not scroll ({scroll:?})"
                            );
                            let controls: Vec<_> = header_controls(form)
                                .iter()
                                .map(|selector| {
                                    let drawn = cx
                                        .debug_bounds(selector)
                                        .unwrap_or_else(|| panic!("{at}: {selector} is drawn"));
                                    assert!(
                                        drawn.left() >= header.left() - px(0.01)
                                            && drawn.right() <= header.right() + px(0.01)
                                            && drawn.bottom() <= region.bottom() + px(0.01),
                                        "{at}: step {step}: {selector} {drawn:?} lies inside its header {header:?}"
                                    );
                                    drawn
                                })
                                .collect();
                            let first = placed.get_or_insert_with(|| controls.clone());
                            assert_eq!(
                                &controls, first,
                                "{at}: step {step}: the controls keep their places"
                            );
                        }
                        show(&documents[0], 0);
                    }
                }
            }
            cx.update(|window, cx| {
                window.set_scale_factor(2.);
                appearance::apply_text_sizes(13, 12, window, cx)
            });
            eprintln!(
                "narrowest compact and full sides (scale, pt, compact px, full px): {boundaries:?}"
            );
            eprintln!("took {:?}", started.elapsed());
        }

        /// A compact header's menus hold what the full header's controls
        /// did, each item named as the control it replaces: the compact
        /// form's zoom menu Fit page width, Zoom out and Zoom in; its "more"
        /// menu Read page text (unavailable while its page is prepared, for
        /// the reason it states, which keeps it in the keys' order), the
        /// link, checked while linked, and the open control; and the
        /// narrowest form's "more" menu the zooms first, then those. Each
        /// menu opens from the keyboard, and focus stays where it was put as
        /// a resize changes the form, full, compact, narrowest and back: Fit's
        /// on the zoom menu, then on the "more" menu, and Zoom out's, the
        /// zoom menu's and Read page text's on the menu that holds them, and
        /// back on each as its form returns.
        #[gpui::test]
        fn a_compact_header_s_menus_hold_its_controls(cx: &mut gpui_kit::TestAppContext) {
            let started = std::time::Instant::now();
            // Each side offers its captured bytes, as the app does here.
            let (content, documents) = both_sides("a compact header's menus", 3);
            let (view, cx) = window(cx, content);
            let id = view.entity_id();
            cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
            // A side of each form's width at 13 pt.
            let resize = |cx: &mut gpui_kit::VisualTestContext, form: HeaderForm| {
                let side = match form {
                    HeaderForm::Full => 500.,
                    HeaderForm::Compact => 260.,
                    HeaderForm::Narrowest => 100.,
                };
                view.update(cx, |view, cx| {
                    view.width = px(side * 2.);
                    cx.notify();
                });
                crate::tags::tests::draw(cx);
                assert_eq!(
                    DRAWN_FORMS.with_borrow(|drawn| drawn[&0]),
                    form,
                    "{side} px"
                );
            };
            let item = |cx: &mut gpui_kit::VisualTestContext, name: &str, role: &str| {
                cx.debug_bounds(format!("menu-item: {name} · {role}").leak())
            };
            // This module runs on Linux, where the open control is Open in
            // default app; on macOS it is Quick Look's System preview.
            let open = "Open in default app: Before, a read-only copy as a PDF document";
            let zooms = [
                ("Before: Fit page width", "MenuItem"),
                ("Before: Zoom out", "MenuItem"),
                ("Before: Zoom in", "MenuItem"),
            ];
            let page = [
                ("Before: read extracted text from page 1", "MenuItem"),
                (
                    "Before: Pages independent. Link pages to this page",
                    "MenuItemCheckBox False",
                ),
                (open, "MenuItem"),
            ];
            let both: Vec<_> = zooms.iter().chain(&page).copied().collect();
            let reveal = cx
                .update(|_, cx| crate::system_preview::side_header(id, "pdf-side-0", cx))
                .expect("the side's header");
            // Where focus is put, in what form, and each form a resize then
            // takes, with the menu that holds focus there (none where the
            // control itself is drawn again), the key that opens it and the
            // items it holds, in order.
            type Held<'a> = Option<(&'static str, &'static str, &'a [(&'a str, &'a str)])>;
            type Walk<'a> = (ElementId, HeaderForm, Vec<(HeaderForm, Held<'a>)>);
            let walks: [Walk; 4] = [
                (
                    ElementId::from(("pdf-zoom", 1usize)),
                    HeaderForm::Full,
                    vec![
                        (
                            HeaderForm::Compact,
                            Some(("pdf-zoom-menu-0", "space", &zooms[..])),
                        ),
                        (
                            HeaderForm::Narrowest,
                            Some(("pdf-more-0", "enter", &both[..])),
                        ),
                        (HeaderForm::Full, None),
                    ],
                ),
                (
                    ElementId::from(("pdf-zoom", 0usize)),
                    HeaderForm::Full,
                    vec![
                        (
                            HeaderForm::Narrowest,
                            Some(("pdf-more-0", "space", &both[..])),
                        ),
                        (HeaderForm::Full, None),
                    ],
                ),
                (
                    ElementId::from(("pdf-zoom-menu", 0usize)),
                    HeaderForm::Compact,
                    vec![
                        (
                            HeaderForm::Narrowest,
                            Some(("pdf-more-0", "enter", &both[..])),
                        ),
                        (HeaderForm::Compact, None),
                    ],
                ),
                (
                    ElementId::from(("pdf-read-text", 0usize)),
                    HeaderForm::Full,
                    vec![
                        (
                            HeaderForm::Compact,
                            Some(("pdf-more-0", "enter", &page[..])),
                        ),
                        (HeaderForm::Full, None),
                    ],
                ),
            ];
            for (control, form, steps) in walks {
                resize(cx, form);
                cx.update(|window, cx| reveal.focus(control.clone(), cx).focus(window, cx));
                crate::tags::tests::draw(cx);
                for (form, menu) in steps {
                    resize(cx, form);
                    let at = format!("{control:?} in the {form:?} form");
                    assert_eq!(
                        cx.update(|window, _| reveal.focused(window)),
                        Some(control.clone()),
                        "{at}: the focus stays"
                    );
                    let Some((menu, key, items)) = menu else {
                        continue;
                    };
                    assert!(cx.debug_bounds(menu).is_some(), "{at}: {menu} is drawn");
                    // The focused control is the menu: the key opens it.
                    cx.simulate_keystrokes(key);
                    crate::tags::tests::draw(cx);
                    let mut above = None;
                    for (name, role) in items {
                        let drawn = item(cx, name, role)
                            .unwrap_or_else(|| panic!("{at}: {menu} holds {name} · {role}"));
                        assert!(
                            above.is_none_or(|above| drawn.top() > above),
                            "{at}: {name} follows the item before it"
                        );
                        above = Some(drawn.top());
                    }
                    cx.simulate_keystrokes("escape");
                    crate::tags::tests::draw(cx);
                }
            }
            // Read page text while its page is prepared: unavailable, saying
            // why, and the link checked while linked, in both compact forms.
            {
                let mut state = documents[0].state.lock().unwrap();
                state.cache = Cache::default();
                state.pending = true;
            }
            for document in &documents {
                document.linked.store(true, Ordering::Relaxed);
            }
            for form in [HeaderForm::Compact, HeaderForm::Narrowest] {
                resize(cx, form);
                cx.update(|window, cx| {
                    reveal
                        .focus(ElementId::from(("pdf-more", 0usize)), cx)
                        .focus(window, cx)
                });
                cx.simulate_keystrokes("enter");
                crate::tags::tests::draw(cx);
                for (name, role) in [
                    (
                        format!("Before: read extracted text from page 1 · {PREPARING}"),
                        "MenuItem disabled",
                    ),
                    (
                        "Before: Pages linked. Switch to independent pages".to_owned(),
                        "MenuItemCheckBox True",
                    ),
                ] {
                    assert!(
                        item(cx, &name, role).is_some(),
                        "{form:?}: the more menu holds {name} · {role}"
                    );
                }
                cx.simulate_keystrokes("escape");
                crate::tags::tests::draw(cx);
            }
            eprintln!("took {:?}", started.elapsed());
        }

        /// Both sides of a comparison take one header form, the narrower of
        /// the two sides' own, so their headings lie level and they show the
        /// same controls: beside an After of 111 pages, whose short page
        /// number is wider, a Before of 3 takes After's form at every side
        /// width stepped 2 px at a time across both boundaries, from 210 to
        /// 330 px at 13 pt and from 300 to 450 px at 18 pt, at scales 2 and
        /// 1.2, and the two headings' tops are equal. Each side laid out
        /// alone, Before took the compact form while After took the narrowest
        /// in 234 to 260 px sides at 13 pt (322 to 360 px at 18 pt), After's
        /// heading 3.5 px higher at scale 2 (4.5 px at 18 pt), and the full
        /// form beside the compact one at 292 to 304 px (404 to 420 px at
        /// 18 pt). Neither side offers its bytes to another app, as for bytes
        /// the platform offers no app for, so the full form without an open
        /// control stays covered.
        #[gpui::test]
        fn both_sides_of_a_comparison_take_one_form_the_narrower(
            cx: &mut gpui_kit::TestAppContext,
        ) {
            let started = std::time::Instant::now();
            let (content, _documents) = sides_offering("one form", [3, 111], false);
            let (view, cx) = window(cx, content);
            let mut forms = std::collections::BTreeSet::new();
            // Each step's form and heading top below the header's top, where
            // they change as the sides widen.
            let mut table = Vec::new();
            for scale in [2f32, 1.2] {
                cx.update(|window, _| window.set_scale_factor(scale));
                for (text, widths) in [(13u8, 210..=330), (18, 300..=450)] {
                    cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
                    let mut last = None;
                    for side in widths.step_by(2) {
                        view.update(cx, |view, cx| {
                            view.width = px(side as f32 * 2.);
                            cx.notify();
                        });
                        DRAWN_FORMS.with_borrow_mut(|drawn| drawn.clear());
                        crate::tags::tests::draw(cx);
                        let drawn = DRAWN_FORMS.with_borrow(|drawn| [drawn[&0], drawn[&1]]);
                        let header = cx.debug_bounds("pdf-side-header-0").expect("a header");
                        let [before, after] = ["pdf-side-heading-0", "pdf-side-heading-1"]
                            .map(|selector| cx.debug_bounds(selector).expect("a heading"));
                        let at = format!("scale {scale}, {text} pt, side {side} px");
                        assert_eq!(drawn[0], drawn[1], "{at}: Before and After take one form");
                        assert!(
                            (before.top() - after.top()).abs() < px(0.01),
                            "{at}, {:?}: Before's heading {before:?} and After's {after:?} lie level",
                            drawn[0]
                        );
                        forms.insert((text, drawn[0]));
                        let state = (drawn[0], f32::from(before.top() - header.top()));
                        if last != Some(state) {
                            table.push((scale, text, side, state));
                            last = Some(state);
                        }
                    }
                }
            }
            cx.update(|window, cx| {
                window.set_scale_factor(2.);
                appearance::apply_text_sizes(13, 12, window, cx)
            });
            for text in [13, 18] {
                for form in [HeaderForm::Full, HeaderForm::Compact, HeaderForm::Narrowest] {
                    assert!(
                        forms.contains(&(text, form)),
                        "at {text} pt the walk crosses into the {form:?} form"
                    );
                }
            }
            eprintln!(
                "from each side width on (scale, pt, side px, (form, heading top)): {table:?}"
            );
            eprintln!("took {:?}", started.elapsed());
        }

        /// What one side's header draws in a frame, as the walk in
        /// [`both_sides_draw_one_header_and_level_pages_at_every_width`]
        /// compares the two.
        #[derive(Clone, Debug, PartialEq)]
        struct DrawnSide {
            form: HeaderForm,
            /// The page number's form: "labelled", "short" or "icon".
            number: &'static str,
            /// The lines its heading and controls lie on.
            lines: usize,
            height: Pixels,
            page: Pixels,
        }

        /// Side `index`'s header as the last frame drew it, and every
        /// labelled control in it whose label was cut short of its whole
        /// text: each run of text behind which the view paints its mark
        /// (`width_walk::text_mark`) inside a labelled control is as wide
        /// as the label laid out whole, where an ellipsis ends a cut one.
        fn drawn_side(
            cx: &mut gpui_kit::VisualTestContext,
            index: usize,
        ) -> (DrawnSide, Vec<String>) {
            let selector = |name: &str| -> &'static str { format!("{name}-{index}").leak() };
            let form = DRAWN_FORMS.with_borrow(|drawn| drawn[&index]);
            let controls = crate::system_preview::DRAWN_CONTROLS.with_borrow(Clone::clone);
            let number = controls[selector("pdf-page-number")].form;
            let parts: Vec<&'static str> = vec![
                selector("pdf-side-heading"),
                selector("pdf-system"),
                selector("pdf-read-text"),
                selector("pdf-back"),
                selector("pdf-page-number"),
                selector("pdf-forward"),
                selector("pdf-link"),
                selector("pdf-zooms"),
                selector("pdf-zoom-menu"),
                selector("pdf-more"),
            ];
            let mut tops: Vec<Pixels> = Vec::new();
            for part in &parts {
                if let Some(bounds) = cx.debug_bounds(part)
                    && !tops.iter().any(|top| (*top - bounds.top()).abs() < px(1.))
                {
                    tops.push(bounds.top());
                }
            }
            let header = cx
                .debug_bounds(selector("pdf-side-header"))
                .expect("a header");
            let page = cx
                .debug_bounds(selector("pdf-scroll"))
                .expect("a page's room");
            // The labelled controls, with what each reads.
            let mut labelled: Vec<(&'static str, String)> = [
                selector("pdf-read-text"),
                selector("pdf-page-number"),
                selector("pdf-link"),
                selector("pdf-zoom-menu"),
            ]
            .into_iter()
            .filter_map(|part| {
                let drawn = controls.get(part)?;
                (drawn.form != "icon").then(|| (part, drawn.shows.clone()))
            })
            .collect();
            if form == HeaderForm::Full {
                labelled.push((["pdf-zoom-1", "pdf-zoom-4"][index], "Fit".to_owned()));
            }
            // This module runs on Linux, whose open control is Open in
            // default app, drawn labelled where its text is drawn.
            let open = selector("pdf-system");
            if cx
                .debug_bounds(format!("{open}/rest/text").leak())
                .is_some()
            {
                labelled.push((open, "Open in default app".to_owned()));
            }
            let labelled: Vec<(&'static str, String, Bounds<Pixels>)> = labelled
                .into_iter()
                .filter_map(|(part, shows)| Some((part, shows, cx.debug_bounds(part)?)))
                .collect();
            let mark = gpui_kit::Background::from(crate::width_walk::text_mark());
            let cut = cx.update(|window, _| {
                let scale = window.scale_factor();
                let quads = window.painted_quads();
                let weight = window.text_style().font_weight;
                labelled
                    .iter()
                    .filter_map(|(part, shows, bounds)| {
                        let painted: Pixels = quads
                            .iter()
                            .filter(|quad| quad.background == mark)
                            .filter(|quad| {
                                let middle = point(
                                    px((quad.bounds.left() + quad.bounds.size.width / 2.).0
                                        / scale),
                                    px((quad.bounds.top() + quad.bounds.size.height / 2.).0
                                        / scale),
                                );
                                bounds.contains(&middle)
                            })
                            .map(|quad| px(quad.bounds.size.width.0 / scale))
                            .fold(Pixels::ZERO, |sum, width| sum + width);
                        let whole = crate::views::shaped_width(
                            shows,
                            appearance::ui_text(12.),
                            weight,
                            window,
                        );
                        (painted < whole - px(0.5)).then(|| {
                            format!(
                                "{part} reads {shows:?} cut, {painted:?} of {whole:?} in {bounds:?}"
                            )
                        })
                    })
                    .collect()
            });
            (
                DrawnSide {
                    form,
                    number,
                    lines: tops.len(),
                    height: header.size.height,
                    page: page.top(),
                },
                cut,
            )
        }

        /// Both sides of a comparison draw one header and lie level at every
        /// width, as the pair lays them out (`pair_layouts`): at scales 1.2,
        /// as Hyprland gives a panel it cannot set to 1.25, and 2, at 13 and
        /// 18 pt, beside each other a Before of 3 pages and an After of 111,
        /// each offering its bytes to another app as the platform does (Open
        /// in default app on Linux, in the full form's first line), the
        /// comparison widened a device pixel at a time wherever either
        /// side's header changes between steps of 4 px and across the side
        /// widths QA-91 named natively (156.5, 184.8 to 185.7, 191.5 to
        /// 192.3 and 314 px), every side: takes the same form as its partner,
        /// draws the page number in the same form (labelled, short or its
        /// icon), its heading and controls on as many lines, a header as tall
        /// and its page's top level with its partner's; a full header lies on
        /// at most [`FULL_LINES`] lines; and no labelled control's label is
        /// cut. Natively at scale 1.2, laid out from each side's own sums of
        /// unrounded widths, After's full header took a fourth line beside
        /// Before's three in 314 px sides (a 956 px window) and "Independent
        /// pages" was cut on both, and in 156.5 px sides Before drew "1/3"
        /// and its run wrapped while After drew the page icon on one line, so
        /// the pages lay 38 device pixels apart. Here, in the tests' font, the
        /// walk found 386 such widths: at 13 pt and scale 1.2, Before's run of
        /// icons wrapped beside After's identical run on one line in 147.08 px
        /// sides (a comparison of 353 device pixels, whose Before frame GPUI
        /// rounds a device pixel narrower), Before drew "1/3" beside After's
        /// icon from 160 px and "Page 1 / 3", cut to "Page 1…", beside After's
        /// "1/111" from 210 px; and at both scales and sizes each side chose
        /// its page number's reading alone.
        #[gpui::test]
        fn both_sides_draw_one_header_and_level_pages_at_every_width(
            cx: &mut gpui_kit::TestAppContext,
        ) {
            let started = std::time::Instant::now();
            let (content, _documents) = sides_offering("level pages", [3, 111], true);
            let (view, cx) = window(cx, content);
            let mut failures: Vec<String> = Vec::new();
            let mut table = Vec::new();
            let mut drawn_count = 0usize;
            for scale in [1.2f32, 2.] {
                cx.update(|window, _| window.set_scale_factor(scale));
                for (text, least, most) in [(13u8, 140f32, 440f32), (18, 190., 600.)] {
                    cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
                    crate::tags::tests::draw(cx);
                    // The comparison `device` device pixels wide, each side
                    // half of it, as the app's comparison is laid out.
                    let mut at = |cx: &mut gpui_kit::VisualTestContext, device: i32| {
                        drawn_count += 1;
                        view.update(cx, |view, cx| {
                            view.width = px(device as f32 / scale);
                            cx.notify();
                        });
                        DRAWN_FORMS.with_borrow_mut(|drawn| drawn.clear());
                        crate::system_preview::DRAWN_CONTROLS
                            .with_borrow_mut(|drawn| drawn.clear());
                        cx.update(|window, cx| {
                            window.simulate_next_frame(cx);
                            window.draw(cx).clear(cx);
                        });
                        let side = device as f32 / scale / 2.;
                        let [(before, before_cut), (after, after_cut)] =
                            [0, 1].map(|index| drawn_side(cx, index));
                        let place = format!("scale {scale}, {text} pt, side {side:.2} px");
                        let mut fail = |what: String| failures.push(format!("{place}: {what}"));
                        if before.form != after.form {
                            fail(format!("forms {:?} beside {:?}", before.form, after.form));
                        } else if before.number != after.number {
                            fail(format!(
                                "page numbers {} beside {}",
                                before.number, after.number
                            ));
                        } else if before.lines != after.lines {
                            fail(format!("{} lines beside {}", before.lines, after.lines));
                        }
                        if (before.height - after.height).abs() > px(0.01)
                            || (before.page - after.page).abs() > px(0.01)
                        {
                            fail(format!("not level: {before:?} beside {after:?}"));
                        }
                        for side in [&before, &after] {
                            if side.form == HeaderForm::Full && side.lines > FULL_LINES {
                                fail(format!("a full header on {} lines", side.lines));
                            }
                        }
                        for cut in before_cut.into_iter().chain(after_cut) {
                            fail(cut);
                        }
                        (before, after)
                    };
                    let device = |side: f32| (side * 2. * scale).round() as i32;
                    let mut last: Option<(i32, (DrawnSide, DrawnSide))> = None;
                    let mut side = least;
                    while side <= most {
                        let here = device(side);
                        let drawn = at(cx, here);
                        if let Some((there, was)) = &last
                            && *was != drawn
                        {
                            // Something changed between the steps: every
                            // device pixel between them, noting where it
                            // changes.
                            let mut previous = was.clone();
                            for between in there + 1..=here {
                                let now = if between == here {
                                    drawn.clone()
                                } else {
                                    at(cx, between)
                                };
                                if now != previous {
                                    let side = &now.0;
                                    table.push((
                                        scale,
                                        text,
                                        between as f32 / scale / 2.,
                                        side.form,
                                        side.number,
                                        side.lines,
                                    ));
                                    previous = now;
                                }
                            }
                        }
                        last = Some((here, drawn));
                        side += 4.;
                    }
                    if text == 13 {
                        for named in [156.5f32, 184.8, 185.7, 191.5, 192.3, 314.] {
                            for step in -2..=2 {
                                at(cx, device(named) + step);
                            }
                        }
                    }
                }
            }
            cx.update(|window, cx| {
                window.set_scale_factor(2.);
                appearance::apply_text_sizes(13, 12, window, cx)
            });
            eprintln!(
                "from each side width on (scale, pt, side px, form, page number, lines): {table:?}"
            );
            eprintln!("{drawn_count} widths drawn in {:?}", started.elapsed());
            assert!(
                failures.is_empty(),
                "{} widths where the sides differ:\n{}",
                failures.len(),
                failures.join("\n")
            );
        }

        /// Both sides of a comparison read their kind, "· PDF", or neither:
        /// two paged PDFs side by side, at scales 1.2 and 2, at 13 and 18 pt,
        /// in a 461 px window's 87 px sides and at every side width from 60
        /// to 200 px every other device pixel, are headed "Before · PDF"
        /// beside "After · PDF" or "Before" beside "After", never one beside
        /// the other, across the band where After's heading would fit its
        /// line whole and Before's, the wider, would not. Each side once
        /// dropped its kind alone, and in a 461 × 490 window the
        /// two were headed "Before" beside "After · PDF".
        #[gpui::test]
        fn both_sides_read_their_kind_or_neither(cx: &mut gpui_kit::TestAppContext) {
            let started = std::time::Instant::now();
            let (content, documents) = both_sides("headings of one kind", 3);
            let (view, cx) = window(cx, content);
            // A side that holds a PDF, as each does here.
            let mut holding = rich_preview::Side::unavailable(Path::new("a.pdf"), true, None);
            holding.pdf = Some(documents[0].clone());
            let mut failures = Vec::new();
            let mut band = std::collections::BTreeSet::new();
            for scale in [1.2f32, 2.] {
                cx.update(|window, _| window.set_scale_factor(scale));
                for text in [13u8, 18] {
                    cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
                    crate::tags::tests::draw(cx);
                    let devices = ((60. * 2. * scale) as i32..=(200. * 2. * scale) as i32)
                        .step_by(2)
                        .chain([(87. * 2. * scale) as i32]);
                    for device in devices {
                        let width = px(device as f32 / scale);
                        view.update(cx, |view, cx| {
                            view.width = width;
                            cx.notify();
                        });
                        cx.update(|window, cx| {
                            window.simulate_next_frame(cx);
                            window.draw(cx).clear(cx);
                        });
                        let kinds = [
                            ("pdf-side-heading-0", "Before"),
                            ("pdf-side-heading-1", "After"),
                        ]
                        .map(|(heading, label)| {
                            let kind = cx
                                .debug_bounds(format!("{heading}/{label} · PDF").leak())
                                .is_some();
                            let name = cx
                                .debug_bounds(format!("{heading}/{label}").leak())
                                .is_some();
                            assert!(kind != name, "{heading} reads one heading");
                            kind
                        });
                        let side = width / 2.;
                        if kinds[0] != kinds[1] {
                            failures.push(format!(
                                "scale {scale}, {text} pt, side {side:?}: Before reads its kind {}, After {}",
                                kinds[0], kinds[1]
                            ));
                        }
                        // Where the sides' own headings would differ.
                        let alone = cx.update(|window, _| {
                            let line = pair_line(width, false, false, window);
                            [(0, "Before"), (1, "After")].map(|(index, label)| {
                                side_heading(index, label, &holding, line, true, window).kind
                            })
                        });
                        if alone[0] != alone[1] {
                            band.insert((scale.to_bits(), text));
                        }
                    }
                }
            }
            cx.update(|window, cx| {
                window.set_scale_factor(2.);
                appearance::apply_text_sizes(13, 12, window, cx)
            });
            eprintln!("took {:?}", started.elapsed());
            assert_eq!(
                band.len(),
                4,
                "at both scales and sizes the walk crosses widths where Before's own heading would drop its kind and After's would not: {band:?}"
            );
            assert!(failures.is_empty(), "{}", failures.join("\n"));
        }

        /// An absent side's message wraps within its side rather than be
        /// cut: beside an added PDF, in a 461 px window's 87 px sides at
        /// scales 1.2 and 2, at 13 and 18 pt, "No file on this side" lies
        /// inside its side, in its room beneath the header, on every line it
        /// wraps to there, the room holding them all, and its painted runs
        /// (`width_walk::text_mark`) are its whole words. Its frame once took
        /// no height in its room, so the message showed one line, "No file
        /// on…", with about 280 px free beneath it.
        #[gpui::test]
        fn an_absent_side_s_message_wraps_within_its_side(cx: &mut gpui_kit::TestAppContext) {
            use crate::worker::Content;
            use rich_preview::{Comparison, Side};
            const MESSAGE: &str = "No file on this side";
            let name = Path::new("report.pdf");
            let document = Arc::new(
                Document::new(Arc::from(pdf("an added PDF")), 3, None, || Ok(())).unwrap(),
            );
            show(&document, 0);
            let mut added = Side::unavailable(name, true, None);
            added.pdf = Some(document);
            let (view, cx) = window(
                cx,
                Content::Rich(Comparison {
                    old: Arc::new(Side::unavailable(name, false, None)),
                    new: Arc::new(added),
                }),
            );
            let mark = gpui_kit::Background::from(crate::width_walk::text_mark());
            for scale in [1.2f32, 2.] {
                cx.update(|window, _| window.set_scale_factor(scale));
                for text in [13u8, 18] {
                    cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
                    view.update(cx, |view, cx| {
                        view.width = px(174.);
                        view.height = px(440.);
                        cx.notify();
                    });
                    crate::tags::tests::draw(cx);
                    let at = format!("scale {scale}, {text} pt, 87 px sides");
                    let message = cx.debug_bounds("pdf-side-message-0").expect("the message");
                    let room = cx.debug_bounds("pdf-scroll-0").expect("its room");
                    let (lines, line, runs, whole, space) = cx.update(|window, _| {
                        let size = window.text_style().font_size.to_pixels(window.rem_size());
                        let weight = window.text_style().font_weight;
                        let scale = window.scale_factor();
                        let runs: Vec<Pixels> = window
                            .painted_quads()
                            .into_iter()
                            .filter(|quad| quad.background == mark)
                            .filter(|quad| {
                                message.contains(&point(
                                    px((quad.bounds.left() + quad.bounds.size.width / 2.).0
                                        / scale),
                                    px((quad.bounds.top() + quad.bounds.size.height / 2.).0
                                        / scale),
                                ))
                            })
                            .map(|quad| px(quad.bounds.size.width.0 / scale))
                            .collect();
                        (
                            crate::image_compare::wrapped_lines(
                                MESSAGE,
                                size,
                                message.size.width,
                                window,
                            ),
                            crate::image_compare::text_line(size, window),
                            runs,
                            crate::views::shaped_width(MESSAGE, size, weight, window),
                            crate::views::shaped_width(" ", size, weight, window),
                        )
                    });
                    assert!(
                        lines > 1,
                        "{at}: the message needs more than a line: {message:?}"
                    );
                    assert!(
                        message.left() >= room.left()
                            && message.right() <= room.right() + px(0.01)
                            && message.top() >= room.top()
                            && message.bottom() <= room.bottom() + px(0.01),
                        "{at}: the message {message:?} lies in its side's room {room:?}"
                    );
                    assert!(
                        message.size.height >= line * lines as f32 - px(0.01),
                        "{at}: the message {message:?} shows its {lines} lines of {line:?}"
                    );
                    let painted = runs.iter().fold(Pixels::ZERO, |sum, run| sum + *run);
                    assert!(
                        runs.len() == lines
                            && painted >= whole - space * (lines - 1) as f32 - px(0.5),
                        "{at}: the message's runs {runs:?} are its whole {whole:?} on {lines} lines"
                    );
                }
            }
            cx.update(|window, cx| {
                window.set_scale_factor(2.);
                appearance::apply_text_sizes(13, 12, window, cx)
            });
        }

        /// The room fallback takes the compact form only where its run fits
        /// one line: a side whose full form fits its width but not its room,
        /// and whose compact run wraps, takes the narrowest, never a wrapping
        /// compact form, though that would fit the room. Laid out as a
        /// header the full form fits on three lines, its line about
        /// different page counts beneath them, in lines 150 px wide, where
        /// the compact run, 220 px at 13 pt, does not fit.
        #[gpui::test]
        fn a_short_room_never_brings_a_wrapping_compact_header(cx: &mut gpui_kit::TestAppContext) {
            let (content, _document) = before_only(&pdf("a wrapping compact header"), 3, 0);
            let (_, cx) = window(cx, content);
            cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
            cx.update(|window, _| {
                let parts = |line| HeaderParts {
                    line: px(line),
                    full: FullHeader {
                        heading: px(60.),
                        open: None,
                        page: px(60.),
                        number: px(60.),
                        link: None,
                        zooms: px(140.),
                    },
                    short: px(40.),
                    zoom_menu: px(70.),
                    counts: Some((px(140.), px(30.))),
                };
                let (unscrolled, scrolled) = (parts(150.), parts(130.));
                assert!(
                    full_fits(&unscrolled, window),
                    "the full form fits the width"
                );
                assert!(!compact_fits(&unscrolled, window), "the compact run wraps");
                let height = |form| {
                    header_height(
                        form,
                        &unscrolled,
                        number_room(form, &unscrolled, window).width(&unscrolled),
                        window,
                    )
                };
                let (full, compact, narrowest) = (
                    height(HeaderForm::Full),
                    height(HeaderForm::Compact),
                    height(HeaderForm::Narrowest),
                );
                assert!(
                    narrowest <= compact && compact < full,
                    "a room fits the wrapping compact form and not the full one: \
                     full {full:?}, compact {compact:?}, narrowest {narrowest:?}"
                );
                for room in [compact, full - px(1.)] {
                    let layout =
                        header_layout(&unscrolled, &scrolled, room, HeaderForm::Full, window);
                    assert_eq!(
                        (layout.form, layout.scrolls),
                        (HeaderForm::Narrowest, false),
                        "in {room:?} of room: {layout:?}"
                    );
                }
                // A partner's compact form brings the narrowest too.
                let layout = header_layout(
                    &unscrolled,
                    &scrolled,
                    px(1000.),
                    HeaderForm::Compact,
                    window,
                );
                assert_eq!(
                    layout.form,
                    HeaderForm::Narrowest,
                    "beside a compact partner"
                );
            });
        }

        /// A side without pages beside one with them lays its header out in
        /// its partner's rhythm, so the two headings lie level in every form:
        /// beside the narrowest, whose lines lie a quarter rem from the
        /// header's top, its heading lies a quarter rem from its top too, and
        /// beside the full form half a rem, at 13 and 18 pt and scales 2 and
        /// 1.2, at side widths from 70 to 500 px. In half a rem
        /// beside the narrowest form, an absent After's heading lay 3.25 px
        /// below Before's at 13 pt in a 461 × 490 window.
        #[gpui::test]
        fn an_absent_side_s_heading_lies_level_with_its_partner_s(
            cx: &mut gpui_kit::TestAppContext,
        ) {
            let started = std::time::Instant::now();
            let (content, document) = before_only(&pdf("level headings"), 3, 0);
            // Drawn with its page prepared, so the page lane is never asked
            // for one.
            document
                .state
                .lock()
                .unwrap()
                .cache
                .insert(0, PreparedPage::from(page()));
            let (view, cx) = window(cx, content);
            let mut forms = std::collections::BTreeSet::new();
            for scale in [2f32, 1.2] {
                cx.update(|window, _| window.set_scale_factor(scale));
                for text in [13u8, 18] {
                    cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
                    for side in [70., 100., 140., 180., 220., 260., 300., 400., 500.] {
                        view.update(cx, |view, cx| {
                            view.width = px(side * 2.);
                            cx.notify();
                        });
                        crate::tags::tests::draw(cx);
                        let form = DRAWN_FORMS.with_borrow(|drawn| drawn[&0]);
                        forms.insert((text, form));
                        let [before, after] = ["pdf-side-heading-0", "pdf-side-heading-1"]
                            .map(|selector| cx.debug_bounds(selector).expect("a heading"));
                        assert!(
                            (before.top() - after.top()).abs() < px(0.01),
                            "scale {scale}, {text} pt, side {side} px, {form:?}: \
                             Before's heading {before:?} and After's {after:?} lie level"
                        );
                    }
                }
            }
            cx.update(|window, cx| {
                window.set_scale_factor(2.);
                appearance::apply_text_sizes(13, 12, window, cx)
            });
            // A side with no link reaches its full form before the compact
            // one fits, so the compact form may not show beside an absent
            // side.
            for text in [13, 18] {
                for form in [HeaderForm::Full, HeaderForm::Narrowest] {
                    assert!(
                        forms.contains(&(text, form)),
                        "at {text} pt the walk reaches the {form:?} form"
                    );
                }
            }
            eprintln!("forms reached (pt, form): {forms:?}");
            eprintln!("took {:?}", started.elapsed());
        }

        /// While its page is prepared, Read page text is recorded as it is
        /// drawn (`system_preview::DRAWN_CONTROLS`), labelled and as its
        /// icon: its accessible name, which the control is made and drawn
        /// with, and its tooltip both say why it is unavailable. The record
        /// once held the name and tooltip of the enabled control, which the
        /// drawn one then replaced, so a test reading it saw neither reason.
        #[gpui::test]
        fn read_page_text_is_recorded_as_drawn_while_its_page_prepares(
            cx: &mut gpui_kit::TestAppContext,
        ) {
            let (content, document) = before_only(&pdf("Read page text while preparing"), 3, 1);
            // The page is being prepared, as the page lane holds its request
            // while a slow helper works; set here, so the lane is never
            // asked for it.
            document.state.lock().unwrap().pending = true;
            let (view, cx) = window(cx, content);
            let name = format!("Before: read extracted text from page 2 · {PREPARING}");
            let mut forms = std::collections::BTreeSet::new();
            for width in [900., 200.] {
                view.update(cx, |view, cx| {
                    view.width = px(width);
                    cx.notify();
                });
                crate::system_preview::DRAWN_CONTROLS.with_borrow_mut(|drawn| drawn.clear());
                crate::tags::tests::draw(cx);
                let Some(drawn) = crate::system_preview::DRAWN_CONTROLS
                    .with_borrow(|drawn| drawn.get("pdf-read-text-0").cloned())
                else {
                    // Compact or narrowest: the "more" menu holds it.
                    assert_ne!(DRAWN_FORMS.with_borrow(|drawn| drawn[&0]), HeaderForm::Full);
                    continue;
                };
                assert_eq!(
                    (drawn.name.as_str(), drawn.tooltip.as_deref()),
                    (
                        name.as_str(),
                        Some(format!("Read page text · {PREPARING}").as_str())
                    ),
                    "{width} px: {drawn:?}"
                );
                forms.insert(drawn.form);
            }
            assert_eq!(forms, ["labelled"].into(), "drawn labelled wherever drawn");
        }

        /// While its page is prepared, Read page text reaches an AT-SPI
        /// client disabled, neither enabled nor sensitive, as Previous page
        /// on the first page does, its name saying why; once the page has
        /// arrived it is enabled and sensitive. Its look, place and width
        /// are the ones `read_page_text_is_recorded_as_drawn_while_its_page_prepares`
        /// and `a_side_s_controls_keep_their_places_while_a_page_prepares`
        /// pin.
        #[gpui::test]
        fn read_page_text_is_reported_disabled_while_its_page_prepares(
            cx: &mut gpui_kit::TestAppContext,
        ) {
            use crate::native_accessibility::atspi_state_tests::{next_frame, reached};
            use accesskit_atspi_common::State;
            let (content, document) = before_only(&pdf("Read page text reported disabled"), 3, 1);
            // Being prepared, as the page lane holds its request; set here,
            // so the lane is never asked for it.
            document.state.lock().unwrap().pending = true;
            let (_view, cx) = window(cx, content);
            let usable = State::Enabled | State::Sensitive;
            let state = |cx: &mut gpui_kit::VisualTestContext, name: &str| {
                reached(next_frame(cx))
                    .into_iter()
                    .find(|node| node.name == name)
                    .unwrap_or_else(|| panic!("an AT-SPI client reaches {name:?}"))
                    .state
            };
            let waiting = format!("Before: read extracted text from page 2 · {PREPARING}");
            let state_now = state(cx, &waiting);
            assert!(
                !state_now.contains(State::Enabled) && !state_now.contains(State::Sensitive),
                "while its page is prepared: {state_now:?}"
            );
            {
                let mut state = document.state.lock().unwrap();
                state.cache.insert(1, PreparedPage::from(page()));
                state.pending = false;
            }
            let state_now = state(cx, "Before: read extracted text from page 2");
            assert!(
                state_now.contains(usable),
                "once its page has arrived: {state_now:?}"
            );
        }

        /// A side's heading is measured as layout draws it on this display's
        /// device pixels, so a line as wide as it is drawn holds it and a
        /// line a device pixel narrower does not: at scale 1.2, as Hyprland
        /// gives a panel it cannot set to 1.25, and at each size from 13 to
        /// 18 pt, an absent Before beside an added PDF, its name and its
        /// "Absent" caption each rounded up to a device pixel and the gap
        /// between them on its nearest, and the added After's "After · PDF",
        /// measure what the page view draws. Rounded to whole pixels alone,
        /// the heading measured narrower than drawn, at 15, 16 and 17 pt a
        /// whole device pixel (at 15 pt 107.5 px, 129 device pixels, for
        /// 108.33, 130), so a line a device pixel narrower than drawn was
        /// chosen for the caption and its name cut there.
        #[gpui::test]
        fn a_side_s_heading_is_measured_as_drawn_at_scale_1_2(cx: &mut gpui_kit::TestAppContext) {
            use crate::worker::Content;
            use rich_preview::{Comparison, Side};
            let name = Path::new("report.pdf");
            // The added side with its first page prepared, so the page lane
            // is never asked for one.
            let document = Arc::new(
                Document::new(Arc::from(pdf("an added partner")), 3, None, || Ok(())).unwrap(),
            );
            document
                .state
                .lock()
                .unwrap()
                .cache
                .insert(0, PreparedPage::from(page()));
            let mut added = Side::unavailable(name, true, None);
            added.pdf = Some(document);
            let added = Arc::new(added);
            let absent = Arc::new(Side::unavailable(name, false, None));
            let (_view, cx) = window(
                cx,
                Content::Rich(Comparison {
                    old: absent.clone(),
                    new: added.clone(),
                }),
            );
            let scale = 1.2;
            cx.update(|window, _| window.set_scale_factor(scale));
            let device = px(1. / scale);
            let drawn = |cx: &mut gpui_kit::VisualTestContext, selector: &'static str| {
                cx.debug_bounds(selector)
                    .unwrap_or_else(|| panic!("{selector} is drawn"))
                    .size
                    .width
            };
            // The sizes where whole pixels alone measured the absent side's
            // heading a device pixel or more narrower than it is drawn.
            let mut short = Vec::new();
            for text in 13..=18 {
                cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
                crate::tags::tests::draw(cx);
                assert!(
                    cx.debug_bounds("pdf-side-heading-0/Before").is_some()
                        && cx.debug_bounds("pdf-side-format-0/Absent").is_some(),
                    "at {text} pt the absent Before reads its name and its caption"
                );
                let [before, after] = [
                    drawn(cx, "pdf-side-heading-0"),
                    drawn(cx, "pdf-side-heading-1"),
                ];
                cx.update(|window, _| {
                    let at = format!("at {text} pt and scale {scale}");
                    let heading = |line| side_heading(0, "Before", &absent, line, true, window);
                    let whole = heading(px(1000.));
                    assert!(
                        whole.caption.is_some() && (whole.width - before).abs() < px(0.01),
                        "{at} the absent Before is measured {:?}, as drawn, {before:?}",
                        whole.width
                    );
                    assert!(
                        heading(before).caption.is_some(),
                        "{at} a line as wide as the heading is drawn holds its caption"
                    );
                    let narrower = heading(before - device);
                    assert!(
                        narrower.caption.is_none() && narrower.text == "Before",
                        "{at} a line a device pixel narrower holds the name alone"
                    );
                    let pdf_heading = side_heading(1, "After", &added, px(1000.), true, window);
                    assert!(
                        pdf_heading.text == "After · PDF"
                            && (pdf_heading.width - after).abs() < px(0.01),
                        "{at} \"After · PDF\" is measured {:?}, as drawn, {after:?}",
                        pdf_heading.width
                    );
                    // What whole pixels alone measured the absent side.
                    let size = window.text_style().font_size.to_pixels(window.rem_size());
                    let semibold =
                        crate::views::shaped_width("Before", size, FontWeight::SEMIBOLD, window);
                    let pixels = semibold.ceil()
                        + window.rem_size() * 0.5
                        + crate::views::line_widths(
                            &[("Absent", appearance::ui_text(11.))],
                            window,
                        );
                    // A device pixel short, within the float error of its
                    // sixths of a pixel.
                    if pixels <= before - device + px(0.01) {
                        short.push((text, pixels, before));
                    }
                });
            }
            assert!(
                !short.is_empty(),
                "a size where whole pixels alone measure the heading a device pixel short"
            );
            cx.update(|window, cx| {
                appearance::apply_text_sizes(
                    appearance::DEFAULT_INTERFACE_TEXT_SIZE,
                    12,
                    window,
                    cx,
                )
            });
        }

        /// A side is headed by what it holds, never by its partner: an
        /// absent side reads its name and its "Absent" caption alike beside
        /// an added PDF, whose pages the page view draws, beside a locked
        /// one, which leaves both sides to the generic document surface, and,
        /// as After, beside a deleted one; only a side that holds a PDF reads
        /// "· PDF", the locked one's caption saying "PDF" there. A locked PDF
        /// beside the same PDF with its pages, which the page view draws
        /// both sides of, holds a PDF though none of its pages, so it reads
        /// "After · PDF" with no caption; decided by its pages alone, it
        /// read "After" and "PDF". Beside an added PDF the absent Before once
        /// read "Before · PDF", its partner's format, where beside a locked
        /// one it read "Before" and "Absent".
        #[gpui::test]
        fn an_absent_side_is_headed_by_its_absence_beside_any_partner(
            cx: &mut gpui_kit::TestAppContext,
        ) {
            use crate::worker::Content;
            use rich_preview::{Comparison, Side};
            let fakes = tempfile::tempdir().unwrap();
            let name = Path::new("report.pdf");
            // A side with pages, its first prepared, so the page lane is
            // never asked for one.
            let with_pages = |bytes: Vec<u8>| {
                let document =
                    Arc::new(Document::new(Arc::from(bytes), 3, None, || Ok(())).unwrap());
                document
                    .state
                    .lock()
                    .unwrap()
                    .cache
                    .insert(0, PreparedPage::from(page()));
                let mut side = Side::unavailable(name, true, None);
                side.pdf = Some(document);
                Arc::new(side)
            };
            let absent = || Arc::new(Side::unavailable(name, false, None));
            // A password-protected PDF: its helper reports it locked (kind 6),
            // so it has no pages.
            let bytes = pdf("a locked partner");
            name_helper(
                &bytes,
                &fakes.path().join("pid"),
                &printing(&error_frame(6, 0)),
                LONG,
            );
            let locked = Arc::new(Side::prepare(bytes.clone(), name, || Ok(())).unwrap());
            assert!(locked.pdf.is_none(), "a locked PDF has no pages");
            let (view, cx) = window(
                cx,
                Content::Rich(Comparison {
                    old: absent(),
                    new: with_pages(pdf("an added partner")),
                }),
            );
            let drawn = |cx: &mut gpui_kit::VisualTestContext, selector: String| {
                cx.debug_bounds(selector.leak()).is_some()
            };
            // What side `index` of the page view is headed with: its text,
            // and its caption if it has one.
            let page_view = |cx: &mut gpui_kit::VisualTestContext, index: usize, text: &str| {
                assert!(
                    drawn(cx, format!("pdf-side-heading-{index}/{text}")),
                    "the page view's side {index} reads {text:?}"
                );
                ["Absent", "PDF"]
                    .into_iter()
                    .find(|format| drawn(cx, format!("pdf-side-format-{index}/{format}")))
            };
            crate::tags::tests::draw(cx);
            assert_eq!(
                page_view(cx, 0, "Before"),
                Some("Absent"),
                "beside an added PDF"
            );
            assert_eq!(page_view(cx, 1, "After · PDF"), None, "the added PDF");

            let show = |cx: &mut gpui_kit::VisualTestContext, comparison: Comparison| {
                view.update(cx, |view, cx| {
                    view.content = Content::Rich(comparison);
                    cx.notify();
                });
                crate::tags::tests::draw(cx);
            };
            show(
                cx,
                Comparison {
                    old: with_pages(pdf("a deleted partner")),
                    new: absent(),
                },
            );
            assert_eq!(page_view(cx, 0, "Before · PDF"), None, "the deleted PDF");
            assert_eq!(
                page_view(cx, 1, "After"),
                Some("Absent"),
                "beside a deleted PDF"
            );

            // The same PDF with its pages beside it locked: the page view
            // draws both sides, and the locked one holds a PDF.
            show(
                cx,
                Comparison {
                    old: with_pages(bytes),
                    new: locked.clone(),
                },
            );
            assert_eq!(page_view(cx, 0, "Before · PDF"), None, "the PDF's pages");
            assert_eq!(
                page_view(cx, 1, "After · PDF"),
                None,
                "a locked PDF beside its pages holds a PDF"
            );

            show(
                cx,
                Comparison {
                    old: absent(),
                    new: locked,
                },
            );
            assert!(
                cx.debug_bounds("pdf-side-header-0").is_none(),
                "neither side has pages, so the generic surface draws both"
            );
            for (index, label, format) in [(0, "Before", "Absent"), (1, "After", "PDF")] {
                assert!(
                    cx.debug_bounds(if index == 0 {
                        "rich-side-heading-0"
                    } else {
                        "rich-side-heading-1"
                    })
                    .is_some()
                        && drawn(cx, format!("rich-side-format-{index}/{format}")),
                    "beside a locked PDF, {label} reads {format:?} as its caption"
                );
            }
        }

        /// One frame drawn without running the tasks it starts, so what it
        /// shows is read before the page it asks for can answer.
        fn one_frame(cx: &mut gpui_kit::VisualTestContext) {
            crate::native_accessibility::written::forget();
            cx.update(|window, cx| {
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
            });
        }

        /// A page being prepared says so in one sentence, "Preparing the
        /// page…", in the page's room and as the reason Read page text is
        /// unavailable, in the frame that turns to it, before its request is
        /// made, and in the next, while the page lane prepares it; the page
        /// then arrives in its place. The room once said "Page preparation
        /// pending…" and then "Preparing selected page…" for the one wait,
        /// while Read page text said "Preparing the page…".
        #[gpui::test]
        fn a_page_being_prepared_says_so_in_one_sentence(cx: &mut gpui_kit::TestAppContext) {
            let fakes = tempfile::tempdir().unwrap();
            let bytes = pdf("one sentence for the wait");
            let frame = page_frame(1, 3, Some("the second page"));
            name_helper(&bytes, &fakes.path().join("pid"), &printing(&frame), LONG);
            let (content, document) = before_only(&bytes, 3, 0);
            document
                .state
                .lock()
                .unwrap()
                .cache
                .insert(0, PreparedPage::from(page()));
            let (_view, cx) = window(cx, content);
            crate::tags::tests::draw(cx);
            // Next, as its click does.
            document.select(1, None);
            for (frame, requested) in [("the page turn's frame", false), ("the next frame", true)] {
                assert_eq!(
                    document.state.lock().unwrap().pending,
                    requested,
                    "{frame} is drawn {} the page is asked for",
                    if requested { "after" } else { "before" }
                );
                one_frame(cx);
                let message = crate::native_accessibility::written::labels()
                    .into_iter()
                    .find(|(written, _)| written == "pdf-page-message-0")
                    .and_then(|(_, message)| message);
                assert_eq!(
                    message.as_deref(),
                    Some(PREPARING),
                    "{frame}: the page's room"
                );
                let read = crate::system_preview::DRAWN_CONTROLS
                    .with_borrow(|drawn| drawn["pdf-read-text-0"].clone());
                assert_eq!(
                    read.tooltip,
                    Some(format!("Read page text · {PREPARING}")),
                    "{frame}: Read page text's reason"
                );
            }
            drain_pages();
            crate::tags::tests::draw(cx);
            assert_eq!(page_message(cx, "pdf-page-message-0"), None);
            assert!(document.state.lock().unwrap().cache.get(1).is_some());
        }

        /// The page number's dialog opens with the current page selected in
        /// its field, so the number typed replaces it, as in a native page
        /// field: on page 4 of 12, typing 9 and Enter goes to page 9. The
        /// caret once opened before the number, so typing 9 read 94 and
        /// Enter refused it.
        #[gpui::test]
        fn the_page_number_typed_replaces_the_current_one(cx: &mut gpui_kit::TestAppContext) {
            let (content, document) = before_only(&pdf("a page number typed"), 12, 3);
            // Drawn with its pages prepared, so the page lane is never
            // asked for one.
            document
                .state
                .lock()
                .unwrap()
                .cache
                .insert(3, PreparedPage::from(page()));
            document
                .state
                .lock()
                .unwrap()
                .cache
                .insert(8, PreparedPage::from(page()));
            let (_view, cx) = window(cx, content);
            crate::tags::tests::draw(cx);
            click(cx, "pdf-page-number-0");
            crate::tags::tests::draw(cx);
            assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
            cx.simulate_keystrokes("9");
            crate::tags::tests::draw(cx);
            cx.simulate_keystrokes("enter");
            crate::tags::tests::draw(cx);
            assert!(
                !cx.update(|window, cx| window.has_active_dialog(cx)),
                "Enter accepts the page typed"
            );
            assert_eq!(document.state.lock().unwrap().page, 8, "page 9 is shown");
        }

        /// Why a side shows no pages is static text that an AT-SPI client
        /// reaches: a password-protected PDF's sentence, which its helper
        /// reports for the first page, and an absent side's "No file on this
        /// side". Neither side has pages, so Compare draws them on the
        /// generic document surface, which drew both as plain text that no
        /// assistive technology reached, while the page view's messages were
        /// static text.
        #[gpui::test]
        fn a_locked_pdf_and_an_absent_side_say_why_to_assistive_technology(
            cx: &mut gpui_kit::TestAppContext,
        ) {
            use crate::native_accessibility::{
                atspi_state_tests::{next_frame, reached},
                written,
            };
            let fakes = tempfile::tempdir().unwrap();
            let bytes = pdf("a password-protected PDF");
            // The helper's answer for a locked document (kind 6).
            name_helper(
                &bytes,
                &fakes.path().join("pid"),
                &printing(&error_frame(6, 0)),
                LONG,
            );
            let name = Path::new("locked.pdf");
            let after = rich_preview::Side::prepare(bytes, name, || Ok(())).unwrap();
            assert!(after.pdf.is_none(), "a locked PDF has no pages");
            let locked = "PDF is password protected; use explicit external inspection to unlock it";
            let content = crate::worker::Content::Rich(rich_preview::Comparison {
                old: Arc::new(rich_preview::Side::unavailable(name, false, None)),
                new: Arc::new(after),
            });
            let (_view, cx) = window(cx, content);
            let found = reached(next_frame(cx));
            for sentence in ["No file on this side", locked] {
                assert!(
                    found.iter().any(|node| node.name == sentence),
                    "an AT-SPI client reaches {sentence:?}"
                );
            }
            let labels = written::labels();
            for (id, sentence) in [
                ("rich-side-absent-0", "No file on this side"),
                ("rich-side-message-1", locked),
            ] {
                assert!(
                    labels.contains(&(id.to_owned(), Some(sentence.to_owned()))),
                    "{id} reads {sentence:?} as static text: {labels:?}"
                );
            }
        }

        /// The app's client starts this very executable as the helper with
        /// exactly the helper's arguments, stops it at 10 seconds and shares
        /// the app's two helper slots; and however the helper ends, its
        /// signal never reaches the words: a refused call (SIGSYS) or a kill
        /// the client did not send reads as the renderer stopping, its own
        /// CPU limit or alarm as the timeout.
        #[test]
        fn the_app_s_client_runs_this_executable_and_words_every_ending() {
            use std::os::unix::process::ExitStatusExt;
            let client = Client::new();
            let (command, deadline, slots) = client.parts();
            let (version, parent) = (PROTOCOL_VERSION.to_string(), std::process::id().to_string());
            assert_eq!(
                command.argv(),
                [
                    "/proc/self/exe",
                    crate::pdf_helper::HELPER_FLAG,
                    &version,
                    &parent
                ]
                .map(std::ffi::OsStr::new)
            );
            assert_eq!(deadline, Duration::from_secs(10));
            assert!(std::ptr::eq(slots, &crate::pdf_helper::HELPERS));
            let stopped = "The PDF renderer stopped on this page.";
            let timed_out = "The PDF page took longer than 10 seconds and was stopped.";
            for (signal, sentence) in [
                (libc::SIGSYS, stopped),
                (libc::SIGKILL, stopped),
                (libc::SIGSEGV, stopped),
                (libc::SIGXCPU, timed_out),
                (libc::SIGALRM, timed_out),
            ] {
                let status = std::process::ExitStatus::from_raw(signal);
                let Err(failure) = crate::pdf_helper::classify_exit(status, &[], 0) else {
                    panic!("signal {signal} gave a page");
                };
                assert_eq!(
                    format!("{:#}", super::super::helper::worded(failure)),
                    sentence
                );
            }
        }

        /// What the helper reports is worded only through `Failure`'s
        /// `Display`: a page that lost its text keeps its pixels and says
        /// why in the text dialog; a helper that dies says the renderer
        /// stopped; and a reply that puts a page the view asked for outside
        /// the document contradicts the count the view shows beside it, so
        /// the view shows neither count and calls it unreadable.
        #[test]
        fn the_helper_s_answers_are_worded_by_their_display() {
            let fakes = tempfile::tempdir().unwrap();
            let pid = fakes.path().join("pid");
            let prepare = |name: &str, script: &str, page| {
                let bytes = pdf(name);
                name_helper(&bytes, &pid, script, LONG);
                prepare_page(&bytes, page, &|| Ok(()))
            };
            let textless = prepare("text lost", &printing(&page_frame(0, 2, None)), 0).unwrap();
            assert_eq!(textless.count, 2);
            assert!(textless.page.render.is_some() && textless.page.text.is_none());
            assert_eq!(
                textless.page.text_notice,
                "Page text is unavailable: The PDF renderer stopped on this page."
            );
            let worded = |result: anyhow::Result<Prepared>| match result {
                Ok(_) => panic!("a page"),
                Err(error) => format!("{error:#}"),
            };
            assert_eq!(
                worded(prepare("helper died", "exit 1", 0)),
                "The PDF renderer stopped on this page."
            );
            assert_eq!(
                worded(prepare("page outside", &printing(&error_frame(9, 1)), 2)),
                "The PDF renderer returned an unreadable result."
            );
            assert_eq!(
                worded(prepare("locked", &printing(&error_frame(6, 0)), 2)),
                "PDF is password protected; use explicit external inspection to unlock it"
            );
        }
    }
}
