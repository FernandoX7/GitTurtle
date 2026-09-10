//! Worker-prepared CommonMark/GFM blocks, painted by a bounded native list.
//! Source remains the only text used for copying and staging.
use crate::*;
use markdown::mdast::Node;
use std::ops::Range;

const MAX_SOURCE: usize = 256 * 1024;
const MAX_NODES: usize = 16_384;
const MAX_BLOCKS: usize = 2048;
const MAX_BLOCK_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Default)]
struct Style {
    strong: bool,
    emphasis: bool,
    code: bool,
    deleted: bool,
}
#[derive(Default)]
struct Prose {
    text: String,
    spans: Vec<(Range<usize>, Style)>,
    links: Vec<(String, String)>,
}
enum Kind {
    Prose(Prose),
    Code(String, String),
    Table(Vec<String>, bool),
    Diagram(Arc<rich_preview::Page>),
    Image {
        destination: String,
        alt: String,
        page: Option<Arc<rich_preview::Page>>,
        error: Option<String>,
    },
    Rule,
    Notice(String),
}
struct Block {
    kind: Kind,
    indent: usize,
    heading: u8,
    line: usize,
}
pub struct Document {
    blocks: Vec<Block>,
    pub present: bool,
    pub name: PathBuf,
    origin: Option<gitturtle_core::PreviewAssetScope>,
    repository: Option<GitRepository>,
    source: Arc<str>,
    navigation: std::sync::Mutex<Option<gitturtle_core::HistoryCancellation>>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Origins {
    pub old: Option<gitturtle_core::PreviewAssetScope>,
    pub new: Option<gitturtle_core::PreviewAssetScope>,
}
impl Origins {
    pub fn revisions(old: Option<String>, new: Option<String>) -> Self {
        Self {
            old: old.map(gitturtle_core::PreviewAssetScope::Revision),
            new: new.map(gitturtle_core::PreviewAssetScope::Revision),
        }
    }
}
pub struct Comparison {
    pub old: Arc<Document>,
    pub new: Arc<Document>,
}

impl Document {
    fn image_identities(&self) -> impl Iterator<Item = (Option<&[u8]>, Option<&str>)> {
        self.blocks.iter().filter_map(|block| match &block.kind {
            Kind::Image { page, error, .. } => Some((
                page.as_ref()
                    .and_then(|page| page.render.as_ref())
                    .and_then(|render| render.as_bytes(0)),
                error.as_deref(),
            )),
            _ => None,
        })
    }
    fn notice(name: &std::path::Path, present: bool, message: &str) -> Self {
        Self {
            name: name.into(),
            present,
            origin: None,
            repository: None,
            source: Arc::from(""),
            navigation: std::sync::Mutex::new(None),
            blocks: vec![Block {
                kind: Kind::Notice(message.into()),
                indent: 0,
                heading: 0,
                line: 1,
            }],
        }
    }
    fn bytes(&self) -> usize {
        self.name.as_os_str().len()
            + self.source.len()
            + self
                .blocks
                .iter()
                .map(|block| {
                    std::mem::size_of::<Block>()
                        + match &block.kind {
                            Kind::Prose(prose) => {
                                prose.text.capacity()
                                    + prose.spans.capacity()
                                        * std::mem::size_of::<(Range<usize>, Style)>()
                                    + prose
                                        .links
                                        .iter()
                                        .map(|(a, b)| a.capacity() + b.capacity())
                                        .sum::<usize>()
                            }
                            Kind::Code(language, source) => language.capacity() + source.capacity(),
                            Kind::Table(cells, _) => cells.iter().map(String::capacity).sum(),
                            Kind::Diagram(page) => page_bytes(page),
                            Kind::Image {
                                destination,
                                alt,
                                page,
                                error,
                            } => {
                                destination.capacity()
                                    + alt.capacity()
                                    + page.as_ref().map_or(0, |page| page_bytes(page))
                                    + error.as_ref().map_or(0, String::capacity)
                            }
                            Kind::Notice(message) => message.capacity(),
                            Kind::Rule => 0,
                        }
                })
                .sum::<usize>()
    }
}
fn page_bytes(page: &rich_preview::Page) -> usize {
    page.render
        .as_ref()
        .and_then(|image| image.as_bytes(0))
        .map_or(0, <[u8]>::len)
        + page.error.as_ref().map_or(0, String::capacity)
}
impl Comparison {
    pub fn retained_bytes(&self) -> usize {
        self.old.bytes() + self.new.bytes()
    }
    pub fn cacheable(&self) -> bool {
        [&self.old, &self.new].into_iter().all(|document| {
            document
                .blocks
                .iter()
                .all(|block| !matches!(&block.kind, Kind::Image { error: Some(_), .. }))
        })
    }
    pub fn same_source(&self, other: &Self) -> bool {
        [&self.old, &self.new]
            .into_iter()
            .zip([&other.old, &other.new])
            .all(|(a, b)| {
                if a.source != b.source
                    || a.name != b.name
                    || a.origin != b.origin
                    || a.present != b.present
                {
                    return false;
                }
                a.image_identities().eq(b.image_identities())
            })
    }
    /// The diagrams-only surface shares the already prepared pixels with the
    /// integrated prose view; it never runs a second Mermaid layout/decoder.
    pub fn diagrams(&self) -> Option<rich_preview::Comparison> {
        let side = |document: &Document| {
            let mut side = rich_preview::Side::unavailable(&document.name, document.present, None);
            side.page_kind = "Diagram";
            side.metadata.format = "Mermaid diagrams".into();
            if document.present {
                side.captured = Some(Arc::from(document.source.as_bytes()));
                side.metadata.source = Some(document.source.clone());
            }
            side.pages = document
                .blocks
                .iter()
                .filter_map(|block| {
                    if let Kind::Diagram(page) = &block.kind {
                        Some(rich_preview::Page {
                            render: page.render.clone(),
                            width: page.width,
                            height: page.height,
                            caption: page.caption.clone(),
                            error: page.error.clone(),
                        })
                    } else {
                        None
                    }
                })
                .collect();
            side.page_count = side.pages.len();
            side.metadata.details.push("Up to four bounded Mermaid blocks; Rendered includes surrounding prose and exact Source retains every block.".into());
            Arc::new(side)
        };
        let old = side(&self.old);
        let new = side(&self.new);
        (!old.pages.is_empty() || !new.pages.is_empty())
            .then_some(rich_preview::Comparison { old, new })
    }
}

pub fn is_markdown(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "md" | "markdown" | "mdown"
            )
        })
}

pub fn prepare(
    old: &str,
    new: &str,
    file: &FileChange,
    check: impl Fn() -> anyhow::Result<()>,
) -> anyhow::Result<Option<Comparison>> {
    if ![file.old_path.as_deref(), file.new_path.as_deref()]
        .into_iter()
        .flatten()
        .any(is_markdown)
        || [file.old_mode.as_str(), file.new_mode.as_str()].contains(&"120000")
        || [old, new]
            .iter()
            .any(|source| gitturtle_preview::detect_lfs_pointer(source.as_bytes()).is_some())
    {
        return Ok(None);
    }
    let old_name = file.old_path.as_deref().unwrap_or_else(|| file.path());
    let new_name = file.new_path.as_deref().unwrap_or_else(|| file.path());
    Ok(Some(Comparison {
        old: Arc::new(prepare_document(
            old,
            old_name,
            file.old_path.is_some(),
            &check,
        )?),
        new: Arc::new(prepare_document(
            new,
            new_name,
            file.new_path.is_some(),
            &check,
        )?),
    }))
}

fn prepare_document(
    source: &str,
    name: &std::path::Path,
    present: bool,
    check: &impl Fn() -> anyhow::Result<()>,
) -> anyhow::Result<Document> {
    check()?;
    if !present {
        return Ok(Document::notice(name, false, "No file on this side"));
    }
    if source.len() > MAX_SOURCE {
        return Ok(Document::notice(
            name,
            true,
            "Rendered Markdown supports documents up to 256 KiB. Exact source and diff remain available.",
        ));
    }
    let tree = markdown::to_mdast(source, &markdown::ParseOptions::gfm())
        .map_err(|e| anyhow::anyhow!("Markdown: {e}"))?;
    check()?;
    let mut stack = vec![(&tree, 0usize)];
    let mut nodes = 0;
    let mut references = HashMap::new();
    while let Some((node, depth)) = stack.pop() {
        nodes += 1;
        if let Node::Definition(definition) = node {
            references
                .entry(definition.identifier.to_lowercase())
                .or_insert_with(|| definition.url.clone());
        }
        if nodes > MAX_NODES || depth > 64 {
            return Ok(Document::notice(
                name,
                true,
                "Rendered Markdown exceeds the 16,384-node / 64-level structure limit. Use exact Source.",
            ));
        }
        if nodes.is_multiple_of(256) {
            check()?;
        }
        if let Some(children) = node.children() {
            stack.extend(children.iter().map(|child| (child, depth + 1)));
        }
    }
    let mut builder = Builder {
        blocks: Vec::new(),
        diagrams: 0,
        check,
        source,
        references,
    };
    builder.blocks(&tree, 0, "")?;
    if builder.blocks.len() >= MAX_BLOCKS {
        builder.blocks.truncate(MAX_BLOCKS - 1);
        builder.push(
            Kind::Notice(
                "Rendered block limit reached (2,048). Source contains the complete document."
                    .into(),
            ),
            0,
            0,
            1,
        );
    }
    Ok(Document {
        blocks: builder.blocks,
        present,
        name: name.into(),
        origin: None,
        repository: None,
        source: Arc::from(source),
        navigation: std::sync::Mutex::new(None),
    })
}

/// Called on the repository worker after parsing. Historical image lookup is
/// anchored to full commit identities, never to the current checkout.
pub fn capture_assets(
    comparison: &mut Comparison,
    repo: &GitRepository,
    file: &FileChange,
    origins: &Origins,
    cancellation: &gitturtle_core::HistoryCancellation,
) -> anyhow::Result<()> {
    let check = || {
        anyhow::ensure!(
            !cancellation.is_cancelled(),
            "Markdown local-image capture cancelled"
        );
        Ok::<_, anyhow::Error>(())
    };
    check()?;
    for (document, scope, expected) in [
        (&mut comparison.old, &origins.old, &file.old_oid),
        (&mut comparison.new, &origins.new, &file.new_oid),
    ] {
        check()?;
        let Some(document) = Arc::get_mut(document) else {
            continue;
        };
        document.origin = scope.clone();
        document.repository = Some(repo.clone());
        let all_indices: Vec<_> = document
            .blocks
            .iter()
            .enumerate()
            .filter_map(|(index, block)| matches!(block.kind, Kind::Image { .. }).then_some(index))
            .collect();
        if all_indices.is_empty() {
            continue;
        }
        for &index in all_indices.iter().skip(gitturtle_core::MAX_PREVIEW_ASSETS) {
            if let Kind::Image { page, error, .. } = &mut document.blocks[index].kind {
                *page = None;
                *error = Some(
                    "The 32 local-image limit was reached; exact source remains available.".into(),
                );
            }
        }
        let indices: Vec<_> = all_indices
            .into_iter()
            .take(gitturtle_core::MAX_PREVIEW_ASSETS)
            .collect();
        let Some(scope) = scope else {
            for index in indices {
                if let Kind::Image { page, error, .. } = &mut document.blocks[index].kind {
                    *page = None;
                    *error = Some(
                        "No captured revision/index scope is available for this local image."
                            .into(),
                    );
                }
            }
            continue;
        };
        let destinations: Vec<_> = indices
            .iter()
            .filter_map(|index| {
                if let Kind::Image { destination, .. } = &document.blocks[*index].kind {
                    Some(destination.clone())
                } else {
                    None
                }
            })
            .collect();
        let results = repo.capture_preview_assets(
            scope,
            &document.name,
            &destinations,
            expected.as_deref(),
            4 * 1024 * 1024,
            16 * 1024 * 1024,
            cancellation,
        );
        check()?;
        let mut pixel_budget = 16 * 1024 * 1024usize;
        match results {
            Ok(results) => {
                for (index, result) in indices.into_iter().zip(results) {
                    check()?;
                    let Kind::Image { page, error, .. } = &mut document.blocks[index].kind else {
                        continue;
                    };
                    *page = None;
                    *error = result.error;
                    if let Some(asset) = result.asset {
                        let decoded = (|| {
                            check()?;
                            anyhow::ensure!(
                                pixel_budget > 0,
                                "Markdown images exceed the 16 MiB decoded-pixel budget"
                            );
                            let decoded = gitturtle_preview::decode_image(
                                &asset.bytes,
                                &asset.path.to_string_lossy(),
                                1000,
                            )?;
                            check()?;
                            let bytes = decoded.rgba.len();
                            anyhow::ensure!(
                                bytes <= pixel_budget,
                                "Markdown images exceed the 16 MiB decoded-pixel budget"
                            );
                            let render = worker::render_image(&decoded)?;
                            check()?;
                            pixel_budget -= bytes;
                            Ok::<_, anyhow::Error>(rich_preview::Page {
                                render: Some(render),
                                width: decoded.width,
                                height: decoded.height,
                                caption: None,
                                error: None,
                            })
                        })();
                        check()?;
                        match decoded {
                            Ok(decoded) => {
                                *page = Some(Arc::new(decoded));
                                *error = None;
                            }
                            Err(failure) => *error = Some(format!("{failure:#}")),
                        }
                    }
                }
            }
            Err(failure) => {
                for index in indices {
                    if let Kind::Image { page, error, .. } = &mut document.blocks[index].kind {
                        *page = None;
                        *error = Some(format!("{failure:#}"));
                    }
                }
            }
        }
        if *scope == gitturtle_core::PreviewAssetScope::Worktree
            && document.blocks.len() < MAX_BLOCKS
        {
            document.blocks.push(Block {kind:Kind::Notice("Local images are captured from tracked working files now; this is not an atomic snapshot of the whole worktree.".into()),indent:0,heading:0,line:1});
        }
    }
    check()
}

struct Builder<'a, F> {
    blocks: Vec<Block>,
    diagrams: usize,
    check: &'a F,
    source: &'a str,
    references: HashMap<String, String>,
}
impl<F: Fn() -> anyhow::Result<()>> Builder<'_, F> {
    fn push(&mut self, kind: Kind, indent: usize, heading: u8, line: usize) {
        if self.blocks.len() < MAX_BLOCKS {
            self.blocks.push(Block {
                kind,
                indent: indent.min(8),
                heading,
                line,
            });
        }
    }
    fn blocks(&mut self, node: &Node, indent: usize, prefix: &str) -> anyhow::Result<()> {
        (self.check)()?;
        if self.blocks.len() >= MAX_BLOCKS {
            return Ok(());
        }
        let line = node.position().map_or(1, |position| position.start.line);
        match node {
            Node::Root(root) => {
                for child in &root.children {
                    self.blocks(child, indent, "")?;
                }
            }
            Node::Heading(heading) => self.prose(node, indent, heading.depth, prefix, line),
            Node::Paragraph(_) => self.prose(node, indent, 0, prefix, line),
            Node::Blockquote(quote) => {
                for child in &quote.children {
                    self.blocks(child, indent + 1, "▎ ")?;
                }
            }
            Node::List(list) => {
                for (index, item) in list.children.iter().enumerate() {
                    let prefix = if let Node::ListItem(item) = item
                        && let Some(checked) = item.checked
                    {
                        if checked {
                            "☑ ".into()
                        } else {
                            "☐ ".into()
                        }
                    } else if list.ordered {
                        format!("{}. ", list.start.unwrap_or(1) as usize + index)
                    } else {
                        "• ".into()
                    };
                    if let Some(children) = item.children() {
                        for (index, child) in children.iter().enumerate() {
                            self.blocks(child, indent + 1, if index == 0 { &prefix } else { "" })?;
                        }
                    }
                }
            }
            Node::Code(code) => {
                let language = code.lang.clone().unwrap_or_default();
                if language.eq_ignore_ascii_case("mermaid") {
                    if self.diagrams < gitturtle_preview::mermaid::MAX_DIAGRAMS {
                        self.diagrams += 1;
                        if let Some(document) = gitturtle_preview::mermaid::preview(
                            &code.value,
                            std::path::Path::new("diagram.mmd"),
                            1200,
                            self.check,
                        )? {
                            for diagram in document.diagrams {
                                let (render, width, height) = if let Some(image) = diagram.image {
                                    (
                                        Some(worker::render_image(&image)?),
                                        image.width,
                                        image.height,
                                    )
                                } else {
                                    (None, 1, 1)
                                };
                                self.push(
                                    Kind::Diagram(Arc::new(rich_preview::Page {
                                        render,
                                        width,
                                        height,
                                        caption: Some(format!("Mermaid · source line {line}")),
                                        error: diagram.error,
                                    })),
                                    indent,
                                    0,
                                    line,
                                );
                            }
                        }
                    } else {
                        self.push(Kind::Notice("Mermaid rendering is limited to four diagrams per side; this block is shown as source.".into()),indent,0,line);
                    }
                }
                for (offset, chunk) in code
                    .value
                    .lines()
                    .collect::<Vec<_>>()
                    .chunks(40)
                    .enumerate()
                {
                    self.push(
                        Kind::Code(language.clone(), bounded(&chunk.join("\n"))),
                        indent,
                        0,
                        line + offset * 40,
                    );
                }
            }
            Node::Table(table) => {
                for (index, row) in table.children.iter().enumerate() {
                    if let Some(cells) = row.children() {
                        self.push(
                            Kind::Table(
                                cells
                                    .iter()
                                    .take(12)
                                    .map(|cell| bounded(&cell.to_string()))
                                    .collect(),
                                index == 0,
                            ),
                            indent,
                            0,
                            line + index,
                        );
                    }
                    if row.children().is_some_and(|cells| cells.len() > 12) {
                        self.push(
                            Kind::Notice(
                                "Table columns beyond twelve are available in Source.".into(),
                            ),
                            indent,
                            0,
                            line + index,
                        );
                    }
                }
            }
            Node::ThematicBreak(_) => self.push(Kind::Rule, indent, 0, line),
            Node::Definition(_) => {}
            Node::Html(html) => self.push(
                Kind::Code("HTML shown literally".into(), bounded(&html.value)),
                indent,
                0,
                line,
            ),
            _ => self.push(
                Kind::Notice(format!(
                    "Source construct: {}",
                    bounded(
                        node.position()
                            .and_then(|p| self.source.get(p.start.offset..p.end.offset))
                            .unwrap_or("Use Source to inspect this construct")
                    )
                )),
                indent,
                0,
                line,
            ),
        }
        Ok(())
    }
    fn prose(&mut self, node: &Node, indent: usize, heading: u8, prefix: &str, line: usize) {
        let mut prose = Prose {
            text: prefix.into(),
            ..Default::default()
        };
        let mut images = Vec::new();
        inline(
            node,
            Style::default(),
            &mut prose,
            &mut images,
            &self.references,
        );
        if !prose.text.is_empty() {
            self.push(Kind::Prose(prose), indent, heading, line);
        }
        for (destination, alt) in images {
            self.push(Kind::Image {destination,alt,page:None,error:Some("Local image is unavailable without a captured revision target. Remote resources are never loaded automatically.".into())},indent,0,line);
        }
    }
}
fn bounded(text: &str) -> String {
    if text.len() <= MAX_BLOCK_BYTES {
        return text.into();
    }
    let mut end = MAX_BLOCK_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[Long block continues in Source]", &text[..end])
}
fn inline(
    node: &Node,
    mut style: Style,
    prose: &mut Prose,
    images: &mut Vec<(String, String)>,
    references: &HashMap<String, String>,
) {
    if prose.text.len() >= MAX_BLOCK_BYTES {
        return;
    }
    let text = match node {
        Node::Text(text) => Some(text.value.as_str()),
        Node::InlineCode(code) => {
            style.code = true;
            Some(code.value.as_str())
        }
        Node::Break(_) => Some("\n"),
        Node::Html(html) => Some(html.value.as_str()),
        Node::Strong(_) => {
            style.strong = true;
            None
        }
        Node::Emphasis(_) => {
            style.emphasis = true;
            None
        }
        Node::Delete(_) => {
            style.deleted = true;
            None
        }
        Node::Link(link) => {
            prose
                .links
                .push((bounded(&node.to_string()), link.url.clone()));
            None
        }
        Node::Image(image) => {
            images.push((image.url.clone(), image.alt.clone()));
            Some(image.alt.as_str())
        }
        Node::ImageReference(image) => {
            images.push((
                references
                    .get(&image.identifier.to_lowercase())
                    .cloned()
                    .unwrap_or_else(|| {
                        format!("[unresolved image reference: {}]", image.identifier)
                    }),
                image.alt.clone(),
            ));
            Some(image.alt.as_str())
        }
        Node::LinkReference(link) => {
            prose.links.push((
                node.to_string(),
                references
                    .get(&link.identifier.to_lowercase())
                    .cloned()
                    .unwrap_or_else(|| format!("[unresolved link reference: {}]", link.identifier)),
            ));
            None
        }
        _ => None,
    };
    if let Some(text) = text {
        let start = prose.text.len();
        prose.text.push_str(&bounded(text));
        prose.spans.push((start..prose.text.len(), style));
    }
    if let Some(children) = node.children() {
        for child in children {
            inline(child, style, prose, images, references);
        }
    }
}

pub struct View {
    comparison: Arc<Comparison>,
    lists: [ListState; 2],
    quick: bool,
    focus: FocusHandle,
    active_side: usize,
    linked: bool,
    link_task: Option<Task<()>>,
    depth: usize,
    notice: Option<String>,
}
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Bookmark {
    positions: [(usize, f32); 2],
    linked: bool,
}
impl View {
    pub fn bookmark(&self) -> Bookmark {
        Bookmark {
            positions: self.lists.each_ref().map(|list| {
                let offset = list.logical_scroll_top();
                (offset.item_ix, f32::from(offset.offset_in_item))
            }),
            linked: self.linked,
        }
    }
    pub fn restore(&mut self, bookmark: Bookmark, cx: &mut Context<Self>) {
        self.linked = bookmark.linked;
        for (side, (index, offset)) in bookmark.positions.into_iter().enumerate() {
            let count = if side == 0 {
                self.comparison.old.blocks.len()
            } else {
                self.comparison.new.blocks.len()
            };
            self.lists[side].scroll_to(ListOffset {
                item_ix: index.min(count.saturating_sub(1)),
                offset_in_item: px(if offset.is_finite() {
                    offset.clamp(0., 10000.)
                } else {
                    0.
                }),
            });
        }
        cx.notify();
    }
    pub fn replace(&mut self, comparison: Arc<Comparison>, cx: &mut Context<Self>) {
        if Arc::ptr_eq(&comparison, &self.comparison) {
            return;
        }
        for document in [&self.comparison.old, &self.comparison.new] {
            if let Some(control) = document
                .navigation
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
            {
                control.cancel();
            }
        }
        self.link_task = None;
        let bookmark = self.bookmark();
        self.comparison = comparison;
        self.lists[0].reset(self.comparison.old.blocks.len());
        self.lists[1].reset(self.comparison.new.blocks.len());
        self.restore(bookmark, cx);
    }
    fn follow_link(
        &mut self,
        document: Arc<Document>,
        destination: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if destination.starts_with("https://") || destination.starts_with("http://") {
            open_link(destination, window, cx);
            return;
        }
        if let Some(anchor) = destination.strip_prefix('#') {
            let anchor = anchor.to_lowercase();
            if let Some(index)=document.blocks.iter().position(|block|block.heading>0 && matches!(&block.kind,Kind::Prose(prose) if prose.text.to_lowercase().replace(' ',"-")==anchor)) {
                let side=usize::from(Arc::ptr_eq(&document,&self.comparison.new));self.jump(side,index);
            } else {self.notice=Some("This heading anchor is unavailable in the rendered blocks. Exact Source remains available.".into());}
            cx.notify();
            return;
        }
        if self.depth >= 4 {
            self.notice = Some(
                "Four local documents are open. Close one before following another link.".into(),
            );
            cx.notify();
            return;
        }
        let (Some(repo), Some(scope)) = (document.repository.clone(), document.origin.clone())
        else {
            self.notice=Some("This local link has no captured repository revision. Open it from repository History or Changes.".into());
            cx.notify();
            return;
        };
        let cancellation = gitturtle_core::HistoryCancellation::default();
        if let Some(previous) = document
            .navigation
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .replace(cancellation.clone())
        {
            previous.cancel();
        }
        let destination = destination.to_owned();
        let path = document.name.clone();
        let control = cancellation.clone();
        static LINKS: std::sync::OnceLock<SerialExecutor> = std::sync::OnceLock::new();
        let response=LINKS.get_or_init(||SerialExecutor::new("gitturtle-markdown-links")).submit_read(move|| {
            let mut assets=repo.capture_preview_assets(&scope,&path,&[destination],None,MAX_SOURCE,MAX_SOURCE,&control)?;
            let result=assets.pop().ok_or_else(||anyhow::anyhow!("Local link did not resolve"))?;
            let asset=result.asset.ok_or_else(||anyhow::anyhow!(result.error.unwrap_or_else(||"Local link unavailable".into())))?;
            let source=std::str::from_utf8(&asset.bytes).map_err(|_|anyhow::anyhow!("This local link is not a UTF-8 text document. Use Quick Open for other file types."))?;
            anyhow::ensure!(!source.contains('\0'),"This local link is a binary file. Use Quick Open to inspect it.");
            let check=||{anyhow::ensure!(!control.is_cancelled(),"Local document request cancelled");Ok(())};
            check()?;
            let markdown=if is_markdown(&asset.path) {
                let mut comparison=Comparison {old:Arc::new(Document::notice(&asset.path,false,"No file on this side")),new:Arc::new(prepare_document(source,&asset.path,true,&check)?)};
                let file=FileChange {old_oid:None,new_oid:asset.blob_oid.clone(),old_path:None,new_path:Some(asset.path.clone()),old_mode:"000000".into(),new_mode:asset.mode.clone(),status:gitturtle_core::ChangeStatus::Added};
                capture_assets(&mut comparison,&repo,&file,&Origins {old:None,new:Some(scope)},&control)?;
                Some(Arc::new(comparison))
            }else{None};
            check()?;
            Ok((asset.path,Arc::<str>::from(source),markdown))
        });
        self.notice = Some("Opening captured local document…".into());
        let depth = self.depth + 1;
        self.link_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "Local document read ended without a result"
                ))
            });
            if cancellation.is_cancelled() {
                return;
            }
            let _ = this.update_in(cx, |this, window, cx| {
                this.link_task = None;
                match result {
                    Ok((path, source, markdown)) => {
                        this.notice = None;
                        open_local_document(path, source, markdown, depth, window, cx);
                    }
                    Err(error) => this.notice = Some(format!("{error:#}")),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn jump(&mut self, side: usize, index: usize) {
        for target in 0..2 {
            if target == side || self.linked {
                let count = if target == 0 {
                    self.comparison.old.blocks.len()
                } else {
                    self.comparison.new.blocks.len()
                };
                self.lists[target].scroll_to(ListOffset {
                    item_ix: index.min(count.saturating_sub(1)),
                    offset_in_item: px(0.),
                });
            }
        }
    }
    pub fn new(comparison: Arc<Comparison>, quick: bool, cx: &mut Context<Self>) -> Self {
        let lists = [
            ListState::new(comparison.old.blocks.len(), ListAlignment::Top, px(240.)),
            ListState::new(comparison.new.blocks.len(), ListAlignment::Top, px(240.)),
        ];
        for (side, list) in lists.iter().enumerate() {
            let owner = cx.entity().downgrade();
            list.set_scroll_handler(move |_, _, cx| {
                cx.defer({
                    let owner = owner.clone();
                    move |cx| {
                        let _ = owner.update(cx, |this, cx| {
                            // The toolkit event's visible_range describes the position
                            // before this wheel delta. Read its committed offset after
                            // the list releases its mutable scroll-state borrow.
                            if this.linked {
                                let offset = this.lists[side].logical_scroll_top();
                                let count = if side == 0 {
                                    this.comparison.new.blocks.len()
                                } else {
                                    this.comparison.old.blocks.len()
                                };
                                this.lists[1 - side].scroll_to(ListOffset {
                                    item_ix: offset.item_ix.min(count.saturating_sub(1)),
                                    offset_in_item: offset.offset_in_item,
                                });
                                cx.notify();
                            }
                        });
                    }
                });
            });
        }
        Self {
            comparison,
            lists,
            quick,
            focus: cx.focus_handle(),
            active_side: 1,
            linked: false,
            link_task: None,
            depth: 0,
            notice: None,
        }
    }
    fn block(&mut self, side: usize, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let colors = palette(cx);
        let document = if side == 0 {
            self.comparison.old.clone()
        } else {
            self.comparison.new.clone()
        };
        let block = &document.blocks[index];
        let side_label = if side == 0 {
            "Before"
        } else if self.quick {
            "Source"
        } else {
            "After"
        };
        let mut row = div()
            .id((
                if side == 0 {
                    "markdown-before-block"
                } else {
                    "markdown-after-block"
                },
                index,
            ))
            .w_full()
            .min_w_0()
            .px_3()
            .py_2()
            .pl(px(12. + block.indent as f32 * 12.))
            .text_size(appearance::ui_text(14.))
            .text_color(rgb(colors.text));
        match &block.kind {
            Kind::Prose(prose) => {
                if block.heading > 0 {
                    row = row
                        .font_weight(FontWeight::BOLD)
                        .text_size(appearance::ui_text(match block.heading {
                            1 => 26.,
                            2 => 22.,
                            3 => 18.,
                            _ => 16.,
                        }));
                }
                let spans = prose.spans.iter().map(|(range, style)| {
                    (
                        range.clone(),
                        HighlightStyle {
                            font_weight: style.strong.then_some(FontWeight::BOLD),
                            font_style: style.emphasis.then_some(FontStyle::Italic),
                            background_color: style.code.then_some(rgb(colors.panel).into()),
                            strikethrough: style.deleted.then_some(StrikethroughStyle {
                                thickness: px(1.),
                                color: None,
                            }),
                            ..Default::default()
                        },
                    )
                });
                // StyledText has no accessible node in this toolkit version.
                // Keep the bounded prose on a named child so its link buttons
                // remain separately reachable in the same block.
                let mut text = div()
                    .id("markdown-prose")
                    .role(if block.heading > 0 {
                        Role::Heading
                    } else {
                        Role::Label
                    })
                    .aria_label(prose.text.clone())
                    .child(StyledText::new(prose.text.clone()).with_highlights(spans));
                if block.heading > 0 {
                    text = text.aria_level(block.heading as usize);
                }
                row = row.child(text);
                if !prose.links.is_empty() {
                    row = row.child(div().mt_1().flex().flex_wrap().gap_2().children(
                        prose.links.iter().take(32).enumerate().map(
                            |(link_index, (label, destination))| {
                                let destination = destination.clone();
                                let document = document.clone();
                                button(
                                    ("markdown-link", index * 1024 + link_index),
                                    format!("Open link: {label}"),
                                    "external-link",
                                    false,
                                )
                                .accessibility_label(format!(
                                    "{side_label}: Open link {label}, {destination}"
                                ))
                                .tooltip(destination.clone())
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.follow_link(document.clone(), &destination, window, cx)
                                    },
                                ))
                            },
                        ),
                    ));
                }
            }
            Kind::Code(language, source) => {
                row = row
                    .role(Role::Label)
                    .aria_label(format!(
                        "{language} code, source line {}.\n{source}",
                        block.line
                    ))
                    .bg(rgb(colors.panel))
                    .child(
                        div()
                            .text_size(appearance::ui_text(10.))
                            .text_color(rgb(colors.muted))
                            .child(format!("{language} · line {}", block.line)),
                    )
                    .child(
                        div()
                            .id(("markdown-code", index))
                            .overflow_x_scroll()
                            .font_family("Menlo")
                            .text_size(appearance::code_text())
                            .child(source.clone()),
                    );
            }
            Kind::Table(cells, header) => {
                row = row
                    .role(Role::Label)
                    .aria_label(format!(
                        "Table {}, {}",
                        if *header { "column headings" } else { "row" },
                        cells.join("; ")
                    ))
                    .flex()
                    .p_0()
                    .border_b_1()
                    .border_color(rgb(colors.border));
                for cell in cells {
                    let mut cell = div()
                        .flex_1()
                        .min_w_0()
                        .p_2()
                        .border_r_1()
                        .border_color(rgb(colors.border))
                        .child(cell.clone());
                    if *header {
                        cell = cell.font_weight(FontWeight::BOLD).bg(rgb(colors.panel));
                    }
                    row = row.child(cell);
                }
            }
            Kind::Diagram(page) => {
                row = row
                    .role(Role::Image)
                    .aria_label(format!(
                        "Mermaid diagram, source line {}. {} The diagram source follows below.",
                        block.line,
                        page.error.as_deref().unwrap_or("Rendered diagram.")
                    ))
                    .child(
                        div()
                            .text_size(appearance::ui_text(11.))
                            .child(format!("Mermaid diagram · source line {}", block.line)),
                    );
                if let Some(render) = &page.render {
                    row = row.child(
                        div()
                            .w_full()
                            .aspect_ratio(page.width as f32 / page.height as f32)
                            .child(gif_playback::static_image(render.clone())),
                    );
                }
                if let Some(error) = &page.error {
                    row = row.child(error.clone());
                }
            }
            Kind::Image {
                destination,
                alt,
                page,
                error,
            } => {
                let label = if alt.is_empty() { destination } else { alt };
                row = row
                    .role(Role::Image)
                    .aria_label(format!(
                        "{side_label} image: {label}. {}",
                        error.as_deref().unwrap_or("")
                    ))
                    .child(
                        div()
                            .text_size(appearance::ui_text(11.))
                            .child(format!("Image: {alt}")),
                    );
                if let Some(page) = page
                    && let Some(render) = &page.render
                {
                    row = row.child(
                        div()
                            .w_full()
                            .aspect_ratio(page.width as f32 / page.height as f32)
                            .child(gif_playback::static_image(render.clone())),
                    );
                } else {
                    row = row.child(format!(
                        "{destination} · {}",
                        error.as_deref().unwrap_or("Image unavailable")
                    ));
                }
            }
            Kind::Rule => {
                row = row
                    .role(Role::Label)
                    .aria_label("Section break")
                    .child(div().w_full().h(px(1.)).bg(rgb(colors.border)));
            }
            Kind::Notice(message) => {
                row = row
                    .role(Role::Label)
                    .aria_label(message.clone())
                    .text_color(rgb(colors.muted))
                    .child(message.clone());
            }
        }
        row.into_any_element()
    }
}
impl Drop for View {
    fn drop(&mut self) {
        for document in [&self.comparison.old, &self.comparison.new] {
            if let Some(control) = document
                .navigation
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
            {
                control.cancel();
            }
        }
    }
}
pub(super) fn pause(content: Option<&Content>) {
    if let Some(Content::Text {
        markdown: Some(comparison),
        ..
    }) = content
    {
        for document in [&comparison.old, &comparison.new] {
            if let Some(control) = document
                .navigation
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
            {
                control.cancel();
            }
        }
    }
}
fn open_local_document(
    path: PathBuf,
    source: Arc<str>,
    markdown: Option<Arc<Comparison>>,
    depth: usize,
    window: &mut Window,
    cx: &mut App,
) -> Entity<LocalDocument> {
    let view = cx.new(|cx| LocalDocument {
        source,
        editor: None,
        rendered: markdown.map(|markdown| {
            cx.new(|cx| {
                let mut view = View::new(markdown, true, cx);
                view.depth = depth;
                view
            })
        }),
        show_rendered: true,
    });
    let document = view.clone();
    window.open_alert_dialog(cx, move |dialog, _, _| {
        dialog
            .title(format!("Captured local document · {}", path.display()))
            .width(px(900.))
            .child(document.clone())
            .footer(gpui_kit::component::dialog::DialogFooter::new().child(
                button("close-local-markdown", "Back to document", "", false).on_click(
                    |_, window, cx| {
                        window.close_dialog(cx);
                        window.refresh();
                    },
                ),
            ))
    });
    let focus = view.clone();
    window.defer(cx, move |window, cx| {
        focus.update(cx, |this, cx| this.focus_visible(window, cx));
    });
    view
}
struct LocalDocument {
    source: Arc<str>,
    editor: Option<Entity<EditorState>>,
    rendered: Option<Entity<View>>,
    show_rendered: bool,
}
impl LocalDocument {
    fn focus_visible(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.show_rendered
            && let Some(rendered) = &self.rendered
        {
            rendered.read(cx).focus.clone().focus(window, cx);
        } else {
            let editor = self
                .editor
                .get_or_insert_with(|| text::editor(&self.source, "markdown", None, window, cx));
            editor.focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }
    fn show_source(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_rendered = false;
        self.focus_visible(window, cx);
    }
    fn show_rendered(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_rendered = true;
        self.focus_visible(window, cx);
    }
}
impl Render for LocalDocument {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let source = !self.show_rendered || self.rendered.is_none();
        if source && self.editor.is_none() {
            self.editor = Some(text::editor(&self.source, "markdown", None, window, cx));
        }
        div()
            .h((window.viewport_size().height * 0.65).min(px(700.)))
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        div()
                            .debug_selector(|| "local-document-source-toggle".into())
                            .child(
                                button("local-document-source", "Exact source", "", source)
                                    .toggled(source)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.show_source(window, cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .debug_selector(|| "local-document-rendered-toggle".into())
                            .child(
                                button("local-document-rendered", "Rendered", "", !source)
                                    .toggled(!source)
                                    .disabled(self.rendered.is_none())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.show_rendered(window, cx);
                                    })),
                            ),
                    ),
            )
            .child(div().flex_1().min_h_0().child(if source {
                crate::editor_find::Editor::new(self.editor.as_ref().unwrap())
                    .readonly(true)
                    .aria_label("Captured local document exact source")
                    .h_full()
                    .into_any_element()
            } else {
                self.rendered.as_ref().unwrap().clone().into_any_element()
            }))
    }
}
fn open_link(destination: &str, window: &mut Window, cx: &mut App) {
    let external = destination.starts_with("https://") || destination.starts_with("http://");
    let destination = destination.to_owned();
    window.open_alert_dialog(cx,move|dialog,_,_| {
        let target=destination.clone();dialog.title(if external {"Open external link"}else{"Local Markdown link"}).child(destination.clone())
            .child(if external {"This opens your browser."}else{"This destination is local to the captured document. Use Quick Open to inspect repository files; no external application will be opened."})
            .footer(gpui_kit::component::dialog::DialogFooter::new().child(button("cancel-markdown-link","Close","",false).on_click(|_,window,cx|window.close_dialog(cx)))
                .child(button("open-markdown-link","Open in browser","external-link",true).disabled(!external).on_click(move|_,window,cx| {cx.open_url(&target);window.close_dialog(cx);})))
    });
}
impl Render for View {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = palette(cx);
        div()
            .id("markdown-comparison")
            .role(Role::Group)
            .aria_label("Rendered Markdown preview")
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                let modifiers = &event.keystroke.modifiers;
                if modifiers.platform || modifiers.control || modifiers.alt { return; }
                let side = this.active_side;
                let current = this.lists[side].logical_scroll_top().item_ix;
                let count = if side == 0 { this.comparison.old.blocks.len() } else { this.comparison.new.blocks.len() };
                let index = match event.keystroke.key.as_str() {
                    "up" => current.saturating_sub(1),
                    "down" => current.saturating_add(1),
                    "pageup" => current.saturating_sub(6),
                    "pagedown" => current.saturating_add(6),
                    "home" => 0,
                    "end" => count.saturating_sub(1),
                    _ => return,
                };
                this.jump(side, index);
                cx.stop_propagation();
                cx.notify();
            }))
            .flex()
            .children((0..2).filter(|side| !self.quick || *side == 1).map(|side| {
                let document = if side == 0 {
                    &self.comparison.old
                } else {
                    &self.comparison.new
                };
                let label = if side == 0 {
                    "Before"
                } else if self.quick {
                    "Source"
                } else {
                    "After"
                };
                let mut header = div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .p_2()
                    .border_b_1()
                    .border_color(rgb(colors.border))
                    .child(format!("{label} · Markdown"))
                    .child(button(("markdown-read",side),"Read","",false)
                        .accessibility_label(format!("Read {label} Markdown. Arrow keys move blocks; Page Up and Page Down move six blocks; Home and End move to the document boundaries."))
                        .on_click(cx.listener(move |this,_,window,cx| {this.active_side=side;this.focus.focus(window,cx);cx.notify();})));
                for (name, last) in [("Top", false), ("End", true)] {
                    header = header.child(
                        button(
                            ("markdown-edge", side * 2 + usize::from(last)),
                            name,
                            "",
                            false,
                        )
                        .accessibility_label(format!("{label}: {name}"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let index = if last {
                                if side == 0 {
                                    this.comparison.old.blocks.len()
                                } else {
                                    this.comparison.new.blocks.len()
                                }
                                .saturating_sub(1)
                            } else {
                                0
                            };
                            this.jump(side, index);
                            cx.notify();
                        })),
                    );
                }
                if !self.quick {
                    header = header.child(
                        button(
                            ("markdown-link-scroll", side),
                            if self.linked {
                                "Linked blocks"
                            } else {
                                "Independent scroll"
                            },
                            "",
                            self.linked,
                        )
                        .toggled(self.linked)
                        .accessibility_label(format!("{label}: Link Markdown block scrolling"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.linked = !this.linked;
                            if this.linked {
                                this.jump(side, this.lists[side].logical_scroll_top().item_ix);
                            }
                            cx.notify();
                        })),
                    );
                }
                let owner = cx.entity().downgrade();
                let list = gpui_kit::list(self.lists[side].clone(), move |index, _, cx| {
                    owner
                        .update(cx, |this, cx| this.block(side, index, cx))
                        .unwrap_or_else(|_| div().into_any_element())
                })
                .size_full();
                if let Some(notice) = &self.notice {
                    header = header.child(
                        div()
                            .id(("markdown-notice", side))
                            .role(Role::Status)
                            .aria_label(notice.clone())
                            .a11y_synthetic_children(native_accessibility::polite)
                            .text_size(appearance::ui_text(11.))
                            .child(notice.clone()),
                    );
                }
                if !document.present {
                    header = header.child("Absent");
                }
                div()
                    .id(("markdown-side", side))
                    .role(Role::Document)
                    .aria_label(format!(
                        "{label} Markdown: {}{}",
                        document.name.display(),
                        if document.present {
                            ""
                        } else {
                            ". File absent"
                        }
                    ))
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .border_r_1()
                    .border_color(rgb(colors.border))
                    .child(header)
                    .child(div().flex_1().min_h_0().on_mouse_down(MouseButton::Left,cx.listener(move |this,_,window,cx| {this.active_side=side;this.focus.focus(window,cx);})).child(list))
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    #[gpui::test]
    fn captured_document_source_find_keeps_modal_focus_and_repository_context(
        cx: &mut TestAppContext,
    ) {
        use std::{cell::RefCell, rc::Rc};
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let output = captured.clone();
        cx.update(|cx| {
            gpui_kit::init(cx);
            image_lifetime::init(cx);
            cx.bind_keys([
                KeyBinding::new("cmd-f", Search, Some("GitTurtleList")),
                KeyBinding::new("escape", ClearSearch, Some("GitTurtle")),
            ]);
        });
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| {
                let mut app = GitTurtle::new(
                    None,
                    Preferences::default(),
                    repository_tabs::Session::default(),
                    activity::State::default(),
                    recovery_drafts::State::default(),
                    window,
                    cx,
                );
                app.page = AppPage::Repository;
                app.mode = WorkspaceMode::Compare;
                app
            });
            *output.borrow_mut() = Some(app.clone());
            gpui_kit::component::Root::new(app, window, cx)
        });
        let app = captured.borrow().as_ref().unwrap().clone();
        let source: Arc<str> = "# Companion\r\n\r\nCaptured target λ.\r\n".into();
        let path = PathBuf::from("docs/companion.md");
        let markdown = Arc::new(Comparison {
            old: Arc::new(Document::notice(&path, false, "No file on this side")),
            new: Arc::new(prepare_document(&source, &path, true, &|| Ok(())).unwrap()),
        });
        let document = cx.update(|window, cx| {
            app.read(cx).file_focus.clone().focus(window, cx);
            open_local_document(path, source.clone(), Some(markdown), 1, window, cx)
        });
        fn settle(cx: &mut VisualTestContext) {
            for _ in 0..2 {
                cx.update(|window, cx| window.draw(cx).clear(cx));
                cx.run_until_parked();
            }
        }
        settle(cx);
        cx.update(|window, cx| {
            assert!(
                document
                    .read(cx)
                    .rendered
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .focus
                    .is_focused(window)
            );
            assert!(document.read(cx).editor.is_none());
            // Even a fall-through application action must not move the page
            // behind an open dialog or attach focus to background Search.
            app.update(cx, |app, cx| app.search(&Search, window, cx));
            assert!(matches!(app.read(cx).mode, WorkspaceMode::Compare));
            assert!(!app.read(cx).search.focus_handle(cx).is_focused(window));
        });
        let toggle = cx.debug_bounds("local-document-source-toggle").unwrap();
        cx.simulate_click(toggle.center(), Modifiers::default());
        settle(cx);
        let editor = cx.read(|cx| document.read(cx).editor.as_ref().unwrap().clone());
        cx.update(|window, cx| assert!(editor.focus_handle(cx).is_focused(window)));
        cx.simulate_keystrokes("cmd-f");
        cx.simulate_input("Captured");
        settle(cx);
        cx.update(|window, cx| {
            assert!(window.has_active_dialog(cx));
            assert_eq!(editor.read(cx).search_session().query, "Captured");
            assert_eq!(editor.read(cx).value().as_str(), source.as_ref());
            assert!(crate::editor_find::panel_height(&editor, cx) > px(0.));
            assert!(app.read(cx).search.read(cx).value().is_empty());
            assert!(matches!(app.read(cx).mode, WorkspaceMode::Compare));
        });
        cx.simulate_keystrokes("escape");
        settle(cx);
        cx.update(|window, cx| {
            assert!(window.has_active_dialog(cx));
            assert!(editor.focus_handle(cx).is_focused(window));
            assert_eq!(crate::editor_find::panel_height(&editor, cx), px(0.));
        });
        let toggle = cx.debug_bounds("local-document-rendered-toggle").unwrap();
        cx.simulate_click(toggle.center(), Modifiers::default());
        settle(cx);
        cx.update(|window, cx| {
            assert!(
                document
                    .read(cx)
                    .rendered
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .focus
                    .is_focused(window)
            )
        });
        let toggle = cx.debug_bounds("local-document-source-toggle").unwrap();
        cx.simulate_click(toggle.center(), Modifiers::default());
        settle(cx);
        cx.update(|window, cx| {
            assert_eq!(
                document.read(cx).editor.as_ref().unwrap().entity_id(),
                editor.entity_id()
            );
            assert!(editor.focus_handle(cx).is_focused(window));
            assert_eq!(editor.read(cx).search_session().query, "Captured");
        });
        cx.simulate_keystrokes("escape");
        settle(cx);
        cx.update(|window, cx| {
            assert!(!window.has_active_dialog(cx));
            assert!(matches!(app.read(cx).mode, WorkspaceMode::Compare));
            assert!(app.read(cx).search.read(cx).value().is_empty());
        });
    }
    fn git(path: &std::path::Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().into()
    }
    fn image_file(path: &std::path::Path, color: [u8; 4]) {
        image::RgbaImage::from_pixel(3, 2, image::Rgba(color))
            .save(path)
            .unwrap();
    }
    #[test]
    fn historical_assets_use_each_captured_tree_and_mutable_assets_invalidate_content() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path();
        git(path, &["init", "-q", "-b", "main"]);
        git(path, &["config", "user.name", "Markdown Fixture"]);
        git(path, &["config", "user.email", "fixture@example.invalid"]);
        std::fs::create_dir(path.join("docs")).unwrap();
        let source = "# Captured image\n\n![Figure][figure]\n\n[figure]: figure.png\n";
        std::fs::write(path.join("docs/readme.md"), source).unwrap();
        image_file(&path.join("docs/figure.png"), [255, 0, 0, 255]);
        git(path, &["add", "."]);
        git(path, &["commit", "-qm", "red"]);
        let before = git(path, &["rev-parse", "HEAD"]);
        image_file(&path.join("docs/figure.png"), [0, 0, 255, 255]);
        git(path, &["add", "."]);
        git(path, &["commit", "-qm", "blue"]);
        let after = git(path, &["rev-parse", "HEAD"]);
        image_file(&path.join("docs/figure.png"), [0, 255, 0, 255]);
        let index = std::fs::read(path.join(".git/index")).unwrap();
        let repo = GitRepository::open(path).unwrap();
        let oid = git(path, &["rev-parse", "HEAD:docs/readme.md"]);
        let file = FileChange {
            old_oid: Some(oid.clone()),
            new_oid: Some(oid),
            old_path: Some("docs/readme.md".into()),
            new_path: Some("docs/readme.md".into()),
            old_mode: "100644".into(),
            new_mode: "100644".into(),
            status: gitturtle_core::ChangeStatus::Modified,
        };
        let control = gitturtle_core::HistoryCancellation::default();
        let mut comparison = prepare(source, source, &file, || Ok(())).unwrap().unwrap();
        capture_assets(
            &mut comparison,
            &repo,
            &file,
            &Origins::revisions(Some(before), Some(after)),
            &control,
        )
        .unwrap();
        let pixel = |document: &Document| {
            document.image_identities().next().unwrap().0.unwrap()[..4].to_vec()
        };
        assert_eq!(pixel(&comparison.old), [0, 0, 255, 255]);
        assert_eq!(pixel(&comparison.new), [255, 0, 0, 255]);
        let origins = Origins {
            old: Some(gitturtle_core::PreviewAssetScope::Index),
            new: Some(gitturtle_core::PreviewAssetScope::Worktree),
        };
        let mut first = prepare(source, source, &file, || Ok(())).unwrap().unwrap();
        capture_assets(&mut first, &repo, &file, &origins, &control).unwrap();
        assert_eq!(pixel(&first.new), [0, 255, 0, 255]);
        image_file(&path.join("docs/figure.png"), [255, 255, 0, 255]);
        let mut next = prepare(source, source, &file, || Ok(())).unwrap().unwrap();
        capture_assets(&mut next, &repo, &file, &origins, &control).unwrap();
        assert!(!first.same_source(&next));
        assert_eq!(std::fs::read(path.join(".git/index")).unwrap(), index);
        control.cancel();
        assert!(capture_assets(&mut next, &repo, &file, &origins, &control).is_err());
    }
    #[test]
    fn gfm_structure_preserves_text_and_never_executes_html() {
        let source = "# Heading\n\nA **bold** and *italic* paragraph.\n\n- [x] Task\n\n| Name | Value |\n| --- | --- |\n| one | two |\n\n<script>alert(1)</script>";
        let document =
            prepare_document(source, std::path::Path::new("README.md"), true, &|| Ok(())).unwrap();
        assert!(document.blocks.iter().any(|b| b.heading == 1));
        assert!(document.blocks.iter().any(|b|matches!(&b.kind,Kind::Prose(p) if p.text.contains("bold") && p.spans.iter().any(|(_,s)|s.strong))));
        assert!(
            document
                .blocks
                .iter()
                .any(|b| matches!(&b.kind,Kind::Table(cells,false) if cells==&["one","two"]))
        );
        assert!(
            document
                .blocks
                .iter()
                .any(|b| matches!(&b.kind,Kind::Code(_,literal) if literal.contains("<script>")))
        );
    }
    #[test]
    fn bounded_documents_keep_unavailable_assets_explicit() {
        let source = "![remote](https://example.invalid/image.png)\n\n![escape](../../outside.png)";
        let document = prepare_document(
            source,
            std::path::Path::new("docs/README.md"),
            true,
            &|| Ok(()),
        )
        .unwrap();
        assert_eq!(
            document
                .blocks
                .iter()
                .filter(|b| matches!(&b.kind, Kind::Image { page: None, .. }))
                .count(),
            2
        );
        let document = prepare_document(
            &"a".repeat(MAX_SOURCE + 1),
            std::path::Path::new("large.md"),
            true,
            &|| Ok(()),
        )
        .unwrap();
        assert!(matches!(&document.blocks[0].kind, Kind::Notice(_)));
        assert!(
            prepare_document(
                "# test",
                std::path::Path::new("x.md"),
                true,
                &|| anyhow::bail!("cancelled")
            )
            .is_err()
        );
    }
}
