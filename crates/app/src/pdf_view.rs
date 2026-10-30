//! On-demand captured-byte PDF navigation. Native references remain on one
//! serialized render lane; each side retains at most four pages / 16 MiB.
use crate::native_accessibility::LiveRegion as _;
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
    /// Read page text's, Retry page's, the link's at its wider reading and
    /// each side's page number at its widest for the side's count, laid out.
    static READ_TEXT: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static RETRY_PAGE: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static BEFORE_PAGE_NUMBER: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static AFTER_PAGE_NUMBER: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static LINK_PAGES: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
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

/// The comparison's sides, each with its header of page controls; `width`
/// is the comparison's (`rich_preview::render_comparison`), which its open
/// controls' form fits.
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
        let mut header = div().debug_selector(move || format!("pdf-side-header-{index}")).flex().flex_wrap().items_center().gap_2().p_2().child(div().min_h(appearance::ui_size(28.)).flex().items_center().font_weight(FontWeight::SEMIBOLD).child(heading));
        let end = crate::system_preview::HeaderEnd { inset: window.rem_size() * 0.5, bottom: window.rem_size() * 0.5, right_rule: false, border: colors.border };
        let name = if index == 0 { "pdf-side-0" } else { "pdf-side-1" };
        // The header scrolls where a narrow side cannot hold its controls,
        // revealing the one Tab moves onto (`system_preview::header_ends`).
        let held = crate::system_preview::SideHeader::new(name, if index == 0 { "pdf-side-0/scrollbar" } else { "pdf-side-1/scrollbar" }, cx.entity_id(), opener.showing(), window, cx);
        let mut answer = None;
        if let (Some(offer), Some(bytes)) = (side.system_preview, &side.captured) {
            let target = opener.target(("pdf-system",index), label, bytes, &side.name, "PDF", offer);
            header = header.child(target.held_control(offer.form(room, window), &held, cx));
            answer = target.answered(cx);
        }
        let mut body = div().id(("pdf-scroll",index)).flex_1().min_h_0().min_w_0().overflow_scroll().p_3();
        let Some(document) = side.pdf.clone() else {
            let body = body.child(side.error.clone().unwrap_or_else(|| if side.present {"PDF preview unavailable"} else {"No file on this side"}.into()));
            return div().flex_1().min_w_0().h_full().flex().flex_col().child(crate::system_preview::header_ends(name, Some(crate::system_preview::OwnHeader { header: header.into_any_element(), foot: None, padding: window.rem_size(), held }), vec![answer], end, body.into_any_element())).into_any_element();
        };
        let partner = if quick { None } else { other.pdf.clone() };
        let (page, zoom, pending, error, cached) = {
            let mut state = document.state.lock().unwrap_or_else(|e| e.into_inner());
            let page = state.page;
            (page, state.zoom, state.pending, state.error.clone(), state.cache.get(page))
        };
        if let Some(cached)=&cached {
            let prepared=cached.clone();
            let read = if labelled_button_width("Read page text", &READ_TEXT, window) <= line_room { button(("pdf-read-text",index),"Read page text","file-text",false) } else { button(("pdf-read-text",index),"","file-text",false).tooltip("Read page text") };
            header=header.child(held.track(("pdf-read-text",index), read.debug_selector(move || format!("pdf-read-text-{index}")).accessibility_label(format!("{label}: read extracted text from page {}",page+1)).on_click(move|_,window,cx|read_page_text(prepared.clone(),label,page+1,window,cx)), cx));
        }
        if error.is_some() {
            let target=document.clone();
            let retry = if labelled_button_width("Retry page", &RETRY_PAGE, window) <= line_room { button(("pdf-retry",index),"Retry page","refresh",false) } else { button(("pdf-retry",index),"","refresh",false).accessibility_label("Retry page").tooltip("Retry page") };
            let retry = retry.debug_selector(move || format!("pdf-retry-{index}")).on_click(cx.listener(move|_,_,window,cx| {let mut state=target.state.lock().unwrap_or_else(|e|e.into_inner());state.error=None;state.pending=false;state.generation=state.generation.wrapping_add(1);drop(state);window.refresh();cx.notify();}));
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
        // "1/3" where "Page 3 / 3", the number at its widest for the count,
        // does not fit a line of the header, its whole reading its tooltip.
        let whole = format!("Page {} / {}",page+1,document.count);
        let number_labelled = text_button_width(&[format!("Page {0} / {0}",document.count).as_str()], if index == 0 { &BEFORE_PAGE_NUMBER } else { &AFTER_PAGE_NUMBER }, window) <= line_room;
        let number = if number_labelled { button(("pdf-page-number",index), whole, "", false) } else { button(("pdf-page-number",index), format!("{}/{}",page+1,document.count), "", false).tooltip(whole) };
        let number = number.debug_selector(move || format!("pdf-page-number-{index}")).accessibility_label(format!("{label}: page {} of {}. Enter page number",page+1,document.count))
            .on_click(move |_,window,cx|page_picker(target.clone(),linked_partner.clone(),label,window,cx));
        header = header.child(held.track(("pdf-page-number",index), number, cx));
        if let Some(partner) = partner {
            let linked = document.linked.load(Ordering::Relaxed);
            let target = document.clone();
            let reading = if linked {"Linked pages"} else {"Independent pages"};
            let link = if text_button_width(&["Linked pages", "Independent pages"], &LINK_PAGES, window) <= line_room { button(("pdf-link",index), reading, "", linked) } else { button(("pdf-link",index), "", "link", linked).tooltip(reading) };
            header = header.child(held.track(("pdf-link",index), link.debug_selector(move || format!("pdf-link-{index}")).toggled(linked).accessibility_label(format!("{label}: linked PDF page navigation")).on_click(cx.listener(move |_,_,window,cx| {
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
        div().relative().flex_1().min_w_0().h_full().flex().flex_col().border_r_1().border_color(rgb(colors.border)).child(crate::system_preview::header_ends(name, Some(crate::system_preview::OwnHeader { header: header.into_any_element(), foot: None, padding: window.rem_size(), held }), vec![answer], end, body.into_any_element())).child(status)
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
    fn page() -> rich_preview::Page {
        rich_preview::Page {
            render: None,
            width: 1,
            height: 1,
            caption: None,
            error: None,
        }
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
