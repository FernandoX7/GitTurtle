//! On-demand captured-byte PDF navigation. Native references remain on one
//! serialized render lane; each side retains at most four pages / 16 MiB.
use crate::native_accessibility::LiveRegion as _;
use crate::system_preview::HeaderControl;
use crate::*;
use gpui_kit::base::ElementExt;
use gpui_kit::component::dialog::DialogFooter;
use std::{
    collections::VecDeque,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

const CACHE_BYTES: usize = 16 * 1024 * 1024;
const CACHE_PAGES: usize = 4;

fn renderer() -> &'static SerialExecutor {
    static RENDERER: OnceLock<SerialExecutor> = OnceLock::new();
    RENDERER.get_or_init(|| SerialExecutor::new("gitturtle-pdf-pages"))
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
struct PreparedPage {
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
    pub fn new(
        bytes: Arc<[u8]>,
        count: usize,
        first: Option<rich_preview::Page>,
        check: impl Fn() -> anyhow::Result<()>,
    ) -> anyhow::Result<Self> {
        check()?;
        let mut cache = Cache::default();
        if let Some(page) = first {
            cache.insert(
                0,
                PreparedPage::new(
                    page,
                    gitturtle_preview::decode_pdf_page_text(&bytes, 0, &check),
                ),
            );
        }
        check()?;
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
            let mut preview = gitturtle_preview::decode_pdf_page(&bytes, page, check)?;
            check()?;
            let image = preview
                .pages
                .pop()
                .ok_or_else(|| anyhow::anyhow!("PDF page is unavailable"))?;
            let render = worker::render_image(&image)?;
            check()?;
            let text = gitturtle_preview::decode_pdf_page_text(&bytes, page, check);
            check()?;
            Ok(PreparedPage::new(
                rich_preview::Page {
                    width: image.width,
                    height: image.height,
                    render: Some(render),
                    caption: None,
                    error: None,
                },
                text,
            ))
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
    let focus = input.read(cx).focus_handle(cx);
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
    window.on_next_frame(move |window, cx| focus.focus(window, cx));
}

thread_local! {
    /// Each side header's heading, laid out.
    static BEFORE_HEADING: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static AFTER_HEADING: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    /// Read page text's and Retry page's labels, laid out; the link is a
    /// toggle, laid out at both its readings
    /// (`system_preview::HeaderControl::toggle`).
    static READ_TEXT: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static RETRY_PAGE: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    /// Each side's page number, whole and short, at its widest for a count
    /// of so many digits ([`page_number_widths`]), laid out.
    static BEFORE_PAGE_NUMBER: PageNumberWidths = const { std::cell::RefCell::new(None) };
    static AFTER_PAGE_NUMBER: PageNumberWidths = const { std::cell::RefCell::new(None) };
}

/// A side's page number laid out whole and short at its widest, by what it
/// was laid out with and the count's digits.
type PageNumberWidths =
    std::cell::RefCell<Option<(crate::views::LineKey, usize, (Pixels, Pixels))>>;

#[cfg(test)]
thread_local! {
    /// How many times a page number's readings were built, for a test.
    pub(crate) static PAGE_NUMBER_READINGS_BUILT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The room the first line of side `index`'s header leaves its open control
/// in a side `side` wide: the side less its right edge, the header's `p_2`
/// either side, and its semibold `heading` followed by `gap_2`.
fn header_room(index: usize, side: Pixels, heading: &str, window: &Window) -> Pixels {
    let cache = if index == 0 {
        &BEFORE_HEADING
    } else {
        &AFTER_HEADING
    };
    let rem = window.rem_size();
    let text = crate::views::cached_text_width(cache, heading, window, || {
        let size = window.text_style().font_size.to_pixels(rem);
        crate::views::shaped_width(heading, size, FontWeight::SEMIBOLD, window).ceil()
    });
    side - px(1.) - rem * (0.5 * 2. + 0.5) - text
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
/// widest for `count` pages ([`page_number_readings`]), laid out once per
/// font and size for each number of digits, which alone sets the readings:
/// the twenty readings were once built and joined on every frame.
fn page_number_widths(index: usize, count: usize, window: &Window) -> (Pixels, Pixels) {
    let cache = if index == 0 {
        &BEFORE_PAGE_NUMBER
    } else {
        &AFTER_PAGE_NUMBER
    };
    let digits = count.checked_ilog10().map_or(1, |power| power as usize + 1);
    let key = crate::views::LineKey::current(window);
    cache.with(|cache| {
        let mut cache = cache.borrow_mut();
        match &*cache {
            Some((laid_out, measured, widths)) if *laid_out == key && *measured == digits => {
                *widths
            }
            _ => {
                #[cfg(test)]
                PAGE_NUMBER_READINGS_BUILT.with(|built| built.set(built.get() + 1));
                let width = |readings: &[String]| {
                    let readings: Vec<&str> = readings.iter().map(String::as_str).collect();
                    text_button_width_now(&readings, window)
                };
                let [whole, short] = page_number_readings(count);
                let widths = (width(&whole), width(&short));
                *cache = Some((key, digits, widths));
                widths
            }
        }
    })
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
    let side_width = width / if quick { 1. } else { 2. };
    div().size_full().flex().min_w_0().children([("Before", &preview.old, &preview.new), (if quick {"Source"} else {"After"}, &preview.new, &preview.old)].into_iter().enumerate().filter(|(i,_)| !quick || *i == 1).map(|(index,(label, side, other))| {
        let heading = format!("{label} · PDF");
        let room = header_room(index, side_width, &heading, window);
        // A line of the header to itself: a labelled control wider than this
        // draws its icon or a shorter reading rather than be cut at the
        // side's edge, its whole label in its tooltip and its accessible
        // name the one it has labelled.
        let line_room = crate::system_preview::header_line_room(side_width, window.rem_size() * 0.5);
        // The heading's line is as tall as the scaled 28 px controls, so an
        // absent side's header aligns with its partner's.
        // The header's padding beneath and its rule end it beneath the line
        // saying why a click opened nothing, laid out with the header and the
        // page for the side's room, where controls taller than the room above
        // the line scroll (`system_preview::header_ends`).
        // The header's padding, which its floor in `header_ends` counts.
        let padding = rems(0.5);
        let mut header = div().debug_selector(move || format!("pdf-side-header-{index}")).flex().flex_wrap().items_center().gap_2().p(padding).child(div().min_h(appearance::ui_size(28.)).flex().items_center().font_weight(FontWeight::SEMIBOLD).child(heading));
        let end = crate::system_preview::HeaderEnd { inset: window.rem_size() * 0.5, bottom: window.rem_size() * 0.5, right_rule: false, border: colors.border };
        let name = if index == 0 { "pdf-side-0" } else { "pdf-side-1" };
        // The header scrolls where a narrow side cannot hold its controls,
        // revealing the one Tab moves onto (`system_preview::header_ends`),
        // as the Stashes detail around it does each of them.
        let held = crate::system_preview::SideHeader::new(name, if index == 0 { "pdf-side-0/scrollbar" } else { "pdf-side-1/scrollbar" }, opener.showing(), window, cx).within(&opener);
        let mut answer = None;
        if let (Some(offer), Some(bytes)) = (side.system_preview, &side.captured) {
            let target = opener.target(("pdf-system",index), label, bytes, &side.name, "PDF", offer);
            header = header.child(target.held_control(offer.form(room, window), &held, cx));
            answer = target.answered(cx);
        }
        let mut body = div().id(("pdf-scroll",index)).flex_1().min_h_0().min_w_0().overflow_scroll().p_3();
        let Some(document) = side.pdf.clone() else {
            let body = body.child(side.error.clone().unwrap_or_else(|| if side.present {"PDF preview unavailable"} else {"No file on this side"}.into()));
            return div().flex_1().min_w_0().h_full().flex().flex_col().child(crate::system_preview::header_ends(name, Some(crate::system_preview::OwnHeader { header: header.into_any_element(), foot: None, padding, held }), vec![answer], end, body.into_any_element())).into_any_element();
        };
        let partner = if quick { None } else { other.pdf.clone() };
        let (page, zoom, pending, error, cached) = {
            let mut state = document.state.lock().unwrap_or_else(|e| e.into_inner());
            let page = state.page;
            (page, state.zoom, state.pending, state.error.clone(), state.cache.get(page))
        };
        if let Some(cached)=&cached {
            let prepared=cached.clone();
            let read = HeaderControl::new(("pdf-read-text",index), format!("pdf-read-text-{index}"), "Read page text", format!("{label}: read extracted text from page {}",page+1), "file-text", labelled_button_width("Read page text", &READ_TEXT, window)).labelled_with_icon().button(line_room);
            header=header.child(held.track(("pdf-read-text",index), read.on_click(move|_,window,cx|read_page_text(prepared.clone(),label,page+1,window,cx)), cx));
        }
        if error.is_some() {
            let target=document.clone();
            let retry = HeaderControl::new(("pdf-retry",index), format!("pdf-retry-{index}"), "Retry page", format!("{label}: retry page {}",page+1), "refresh", labelled_button_width("Retry page", &RETRY_PAGE, window)).labelled_with_icon().button(line_room);
            let retry = retry.on_click(cx.listener(move|_,_,window,cx| {let mut state=target.state.lock().unwrap_or_else(|e|e.into_inner());state.error=None;state.pending=false;state.generation=state.generation.wrapping_add(1);drop(state);window.refresh();cx.notify();}));
            header=header.child(held.track(("pdf-retry",index), retry, cx));
        }
        for (previous, disabled) in [(true,page == 0),(false,page+1 >= document.count)] {
            let document = document.clone(); let partner = partner.clone();
            let id = if previous {"pdf-back"} else {"pdf-forward"};
            let step = button((id,index), "", if previous {"chevron-left"} else {"chevron-right"}, false)
                .debug_selector(move || format!("{id}-{index}"))
                .accessibility_label(format!("{label}: {} page",if previous {"Previous"} else {"Next"})).disabled(disabled)
                .on_click(cx.listener(move |_,_,window,cx| { document.select(if previous {page.saturating_sub(1)} else {page+1},partner.as_deref()); window.refresh(); cx.notify(); }));
            header = header.child(held.track((id,index), step, cx));
        }
        let target = document.clone(); let linked_partner = partner.clone();
        // "1/3" where "Page 1 / 3" does not fit a line of the header, and
        // its page icon where "1/3" does not either, its whole reading its
        // tooltip; each form measured at its widest for the count.
        let (labelled, short) = page_number_widths(index, document.count, window);
        let number = HeaderControl::new(("pdf-page-number",index), format!("pdf-page-number-{index}"), format!("Page {} / {}",page+1,document.count), format!("{label}: page {} of {}. Enter page number",page+1,document.count), "page", labelled)
            .short(format!("{}/{}",page+1,document.count), short).button(line_room)
            .on_click(move |_,window,cx|page_picker(target.clone(),linked_partner.clone(),label,window,cx));
        header = header.child(held.track(("pdf-page-number",index), number, cx));
        if let Some(partner) = partner {
            let linked = document.linked.load(Ordering::Relaxed);
            let target = document.clone();
            // Named for the state it shows, as the model's link is.
            let name = if linked {"Pages linked. Switch to independent pages"} else {"Pages independent. Link pages to this page"};
            let link = HeaderControl::toggle(("pdf-link",index), format!("pdf-link-{index}"), ["Independent pages", "Linked pages"], format!("{label}: {name}"), "link", linked, window).button(line_room);
            header = header.child(held.track(("pdf-link",index), link.on_click(cx.listener(move |_,_,window,cx| {
                target.linked.store(!linked,Ordering::Relaxed); partner.linked.store(!linked,Ordering::Relaxed);
                if !linked { target.select(page,Some(&partner)); }
                window.refresh(); cx.notify();
            })), cx));
        }
        for (action,text) in [(0i32,"Fit"),(-1,"−"),(1,"+")] {
            let target = document.clone();
            let id = index*3+(action+1) as usize;
            let zoom = button(("pdf-zoom",id),text,"",false).debug_selector(move || format!("pdf-zoom-{id}")).accessibility_label(format!("{label}: {}",match action {-1=>"Zoom out",1=>"Zoom in",_=>"Fit page width"})).on_click(cx.listener(move |_,_,window,cx| {
                let mut state = target.state.lock().unwrap_or_else(|e|e.into_inner()); state.zoom=if action==0 {1.}else{(state.zoom * if action<0 {0.8}else{1.25}).clamp(0.25,4.)}; drop(state); window.refresh(); cx.notify();
            }));
            header = header.child(held.track(("pdf-zoom",id), zoom, cx));
        }
        header = header.child(div().text_size(appearance::ui_text(10.)).child(format!("{:.0}%",zoom*100.)));
        // The position's live region lies outside the header's region, which
        // can scroll it out of view and out of reach of assistive technology
        // while it is still being announced, in a 1 px box at the side's
        // top-left, in the header's padding, where no control or page lies:
        // laid over the whole side, it took AccessKit's hit tests, which try
        // later siblings first, from Fit, −, +, the page number and the page.
        let status = crate::native_accessibility::recorded(div().id(SharedString::from(format!("pdf-position-status-{index}"))).absolute().top_0().left_0().size(px(1.)).role(Role::Status).aria_label(format!("{label}: page {} of {}, {:.0} percent zoom",page+1,document.count,zoom*100.)).polite());
        if let Some(other)=&other.pdf && document.linked.load(Ordering::Relaxed) && other.count!=document.count {
            header=header.child(div().text_size(appearance::ui_text(10.)).child(format!("Different page counts: {} / {}. Each side stops at its last page.",document.count,other.count)));
        }
        let page_number = page + 1;
        let rendered = match side.system_preview.filter(|_| side.captured.is_some()) {
            Some(offer) => format!("Rendered page; {} opens the captured document.", offer.label()),
            None => "Rendered page.".into(),
        };
        if let Some(page) = cached && let Some(render) = &page.render {
            body = body.child(div().id(("pdf-rendered-page",index)).role(Role::Image).aria_label(format!("{label}: PDF page {} of {}. {rendered}",page_number,document.count)).w(relative(zoom)).min_w(px(100.)).aspect_ratio(page.width as f32/page.height as f32).bg(rgb(0xffffff)).child(crate::gif_playback::static_image(render.clone())));
        } else {
            body=body.child(div().text_color(rgb(colors.muted)).child(error.unwrap_or_else(|| if pending {"Preparing selected page…"}else{"Page preparation pending…"}.into())));
        }
        if let Some(other) = &other.pdf && document.linked.load(Ordering::Relaxed) && other.count != document.count {
            body = body.child(div().mt_2().text_size(appearance::ui_text(11.)).child("The revisions have different page counts. Linked navigation stops each side at its own last page."));
        }
        let view=cx.entity().downgrade();
        // Why the last open opened nothing, as the header's last line.
        div().relative().flex_1().min_w_0().h_full().flex().flex_col().border_r_1().border_color(rgb(colors.border)).child(crate::system_preview::header_ends(name, Some(crate::system_preview::OwnHeader { header: header.into_any_element(), foot: None, padding, held }), vec![answer], end, body.into_any_element())).child(status)
            .on_prepaint(move |_,window,cx| { let _=view.update(cx,|_,cx|document.request(window,cx)); }).into_any_element()
    })).into_any_element()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use core::prelude::v1::test;

    /// Waits until every page requested so far has answered; drawing then
    /// handles the answers.
    pub(crate) fn drain_pages() {
        renderer().drain();
    }

    /// Links `document`'s pages to its partner's where they are independent,
    /// and otherwise makes them independent, as its link's click does on
    /// both sides.
    pub(crate) fn flip_link(document: &Document) {
        document.linked.fetch_xor(true, Ordering::Relaxed);
    }

    /// `preview`, a changed PDF's comparison, as the page view draws it
    /// where a build renders pages (macOS, with PDFKit): each side has three
    /// pages, Before's first prepared, so it offers Read page text, and
    /// After's failed, as Linux renders none, so it offers Retry page. Each
    /// side offers to link their pages.
    #[cfg(target_os = "linux")]
    pub(crate) fn paged(preview: &rich_preview::Comparison) -> crate::worker::Content {
        paged_at(preview, 3, 0)
    }

    /// [`paged`] with `count` pages a side, each side showing page `shown`
    /// (counted from 0), which Before has prepared only where it is the
    /// first.
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
            let document = Document::new(bytes, count, first, || Ok(())).expect("a document");
            document.state.lock().unwrap().page = shown;
            paged.pdf = Some(Arc::new(document));
            Arc::new(paged)
        };
        crate::worker::Content::Rich(rich_preview::Comparison {
            old: side(&preview.old, Some(page())),
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
}
