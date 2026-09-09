//! Static Mermaid diagrams from bounded supplied text. Never opens a source,
//! configuration, linked resource, browser, or renderer executable.
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
};

use anyhow::{Result, ensure};
use mermaid_rs_renderer::{
    DiagramKind, LayoutConfig, Theme, compute_layout, parse_mermaid_strict, render_svg,
};

use crate::{ImagePreview, MAX_SVG_BYTES, decode_image};

pub const MAX_DIAGRAMS: usize = 4;
pub const MAX_DIAGRAM_BYTES: usize = 16 * 1024;
pub const MAX_DIAGRAM_LINES: usize = 256;
const MAX_DOCUMENT_BYTES: usize = 2 * 1024 * 1024;
const MAX_NODES: usize = 128;
const MAX_EDGES: usize = 256;

pub struct Diagram {
    pub source_line: usize,
    pub image: Option<ImagePreview>,
    pub error: Option<String>,
}

pub struct Document {
    pub diagrams: Vec<Diagram>,
    pub total: usize,
}

/// `None` means ordinary source with no recognized Mermaid block. Individual
/// parse/render failures stay attached to their source block; cancellation
/// aborts the whole request and leaves the normal worker generation guard intact.
pub fn preview(
    source: &str,
    path: &Path,
    max_edge: u32,
    check: impl Fn() -> Result<()>,
) -> Result<Option<Document>> {
    check()?;
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !matches!(
        extension.as_str(),
        "mmd" | "mermaid" | "md" | "markdown" | "mdown" | "mdx"
    ) {
        return Ok(None);
    }
    ensure!(
        source.len() <= MAX_DOCUMENT_BYTES,
        "Mermaid source document exceeds 2 MiB"
    );
    let blocks = if matches!(extension.as_str(), "mmd" | "mermaid") {
        vec![(1, source, false)]
    } else {
        markdown_blocks(source)
    };
    if blocks.is_empty() {
        return Ok(None);
    }
    let total = blocks.len();
    let mut diagrams = Vec::new();
    for (source_line, block, unclosed) in blocks.into_iter().take(MAX_DIAGRAMS) {
        check()?;
        let rendered = if unclosed {
            Err(anyhow::anyhow!(
                "Mermaid code fence is not closed; use Source to inspect the complete text."
            ))
        } else {
            catch_unwind(AssertUnwindSafe(|| render(block, max_edge, &check)))
                .unwrap_or_else(|_| Err(anyhow::anyhow!("Mermaid renderer could not complete this diagram; literal source remains available.")))
        };
        check()?;
        let (image, error) = match rendered {
            Ok(image) => (Some(image), None),
            Err(error) => (None, Some(format!("{error:#}"))),
        };
        diagrams.push(Diagram {
            source_line,
            image,
            error,
        });
    }
    Ok(Some(Document { diagrams, total }))
}

fn markdown_blocks(source: &str) -> Vec<(usize, &str, bool)> {
    let mut blocks = Vec::new();
    let mut open: Option<(u8, usize, bool, usize, usize)> = None;
    let mut offset = 0;
    for (line, raw) in source.split_inclusive('\n').enumerate() {
        let content = raw.trim_end_matches(['\r', '\n']);
        let indentation = content.bytes().take_while(|b| *b == b' ').count();
        let trimmed = &content[indentation..];
        if indentation <= 3 {
            if let Some((marker, length, mermaid, start, source_line)) = open {
                let run = trimmed.bytes().take_while(|b| *b == marker).count();
                if run >= length && trimmed[run..].trim().is_empty() {
                    if mermaid {
                        blocks.push((source_line, &source[start..offset], false));
                    }
                    open = None;
                }
            } else if let Some(marker @ (b'`' | b'~')) = trimmed.bytes().next() {
                let length = trimmed.bytes().take_while(|b| *b == marker).count();
                if length >= 3 {
                    let info = trimmed[length..].trim();
                    if marker != b'`' || !info.contains('`') {
                        open = Some((
                            marker,
                            length,
                            info.eq_ignore_ascii_case("mermaid"),
                            offset + raw.len(),
                            line + 2,
                        ));
                    }
                }
            }
        }
        offset += raw.len();
    }
    if let Some((_, _, true, start, source_line)) = open {
        blocks.push((source_line, &source[start..], true));
    }
    blocks
}

fn render(source: &str, max_edge: u32, check: &impl Fn() -> Result<()>) -> Result<ImagePreview> {
    ensure!(
        source.len() <= MAX_DIAGRAM_BYTES,
        "Mermaid diagram exceeds the 16 KiB source limit"
    );
    ensure!(
        source.lines().count() <= MAX_DIAGRAM_LINES,
        "Mermaid diagram exceeds the 256-line limit"
    );
    ensure!(
        source
            .chars()
            .all(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t')),
        "Mermaid contains unsupported control characters"
    );
    let mut nesting = [0usize; 3];
    for byte in source.bytes() {
        match byte {
            b'[' => nesting[0] += 1,
            b'(' => nesting[1] += 1,
            b'{' => nesting[2] += 1,
            b']' => nesting[0] = nesting[0].saturating_sub(1),
            b')' => nesting[1] = nesting[1].saturating_sub(1),
            b'}' => nesting[2] = nesting[2].saturating_sub(1),
            _ => {}
        }
        ensure!(
            nesting.iter().sum::<usize>() <= 32,
            "Mermaid exceeds the 32-level syntax nesting limit"
        );
    }
    let lower = source.to_ascii_lowercase();
    ensure!(
        !["%%{", "url(", "@import", "data:", "javascript:", "!["]
            .iter()
            .any(|token| lower.contains(token)),
        "Mermaid configuration directives and embedded or external resources are unavailable; use literal Source"
    );
    for line in source.lines() {
        let trimmed = line.trim();
        ensure!(
            !trimmed.starts_with("click ") && trimmed != "---",
            "Mermaid click actions and frontmatter configuration are unavailable; use literal Source"
        );
    }
    ensure!(
        !source.as_bytes().windows(2).any(|pair| pair[0] == b'<'
            && (pair[1].is_ascii_alphabetic() || matches!(pair[1], b'/' | b'!'))),
        "Mermaid HTML labels and embedded resources are unavailable; use plain text labels"
    );
    // Select the finite grammar subset before invoking upstream parsing, not
    // merely before layout: unsupported types may have different expansion costs.
    let header = source
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("%%"))
        .unwrap_or("")
        .split(|c: char| c.is_whitespace() || c == ';')
        .next()
        .unwrap_or("");
    ensure!(
        matches!(
            header,
            "flowchart"
                | "graph"
                | "sequenceDiagram"
                | "classDiagram"
                | "stateDiagram"
                | "stateDiagram-v2"
                | "erDiagram"
                | "pie"
        ),
        "This Mermaid diagram type is not supported in-app. Supported: flowchart, sequence, class, state, ER, and pie; literal Source remains available."
    );
    // Upstream expands fan-out as a Cartesian product while parsing. Count
    // every ampersand conservatively, including labels, before that allocation;
    // a post-parse edge limit alone would be too late for compact fan-out input.
    if matches!(header, "flowchart" | "graph") {
        ensure!(
            source.bytes().filter(|byte| *byte == b'&').count() <= 32,
            "Mermaid flowchart exceeds the 32-ampersand source limit (including labels); use separate edges or literal Source"
        );
    }
    check()?;
    let parsed = parse_mermaid_strict(source)?;
    ensure!(
        matches!(
            parsed.graph.kind,
            DiagramKind::Flowchart
                | DiagramKind::Sequence
                | DiagramKind::Class
                | DiagramKind::State
                | DiagramKind::Er
                | DiagramKind::Pie
        ),
        "This Mermaid diagram type is not supported in-app. Supported: flowchart, sequence, class, state, ER, and pie; literal Source remains available."
    );
    let graph = &parsed.graph;
    ensure!(
        graph.nodes.len() <= MAX_NODES
            && graph.sequence_participants.len() <= MAX_NODES
            && graph.pie_slices.len() <= MAX_NODES,
        "Mermaid diagram exceeds the 128-node/participant/slice limit"
    );
    ensure!(
        graph.edges.len() <= MAX_EDGES,
        "Mermaid diagram exceeds the 256-edge/message limit"
    );
    ensure!(
        graph.subgraphs.len() <= 32
            && graph.sequence_frames.len() <= 32
            && graph.sequence_notes.len() <= 128
            && graph.state_notes.len() <= 128,
        "Mermaid diagram exceeds the group/note limit"
    );
    ensure!(
        graph.node_links.is_empty(),
        "Mermaid linked nodes and callbacks are unavailable; use literal Source"
    );
    check()?;
    let theme = Theme::mermaid_default();
    let config = LayoutConfig {
        fast_text_metrics: true,
        ..LayoutConfig::default()
    };
    let layout = compute_layout(graph, &theme, &config);
    check()?;
    let svg = render_svg(&layout, &theme, &config);
    ensure!(
        svg.len() <= MAX_SVG_BYTES,
        "Generated Mermaid SVG exceeds the 2 MiB output limit"
    );
    check()?;
    let mut image = decode_image(svg.as_bytes(), "mermaid.svg", max_edge)?;
    image.format = "Mermaid diagram".into();
    check()?;
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_supported_diagrams_to_bounded_pixels() {
        for source in [
            "flowchart LR\nA[Start] --> B{Ready}\nB --> C[Done]",
            "sequenceDiagram\nAlice->>Bob: Hello\nBob-->>Alice: Ready",
            "classDiagram\nAnimal <|-- Duck\nAnimal : +int age",
            "stateDiagram-v2\n[*] --> Ready\nReady --> Done",
            "erDiagram\nCUSTOMER ||--o{ ORDER : places",
            "pie title Work\n\"Code\" : 60\n\"Review\" : 40",
        ] {
            let rendered = preview(source, Path::new("diagram.mmd"), 800, || Ok(()))
                .unwrap()
                .unwrap();
            let diagram = &rendered.diagrams[0];
            assert!(diagram.error.is_none(), "{source}: {:?}", diagram.error);
            let image = diagram.image.as_ref().unwrap();
            assert!(image.width > 0 && image.height > 0 && image.width.max(image.height) <= 800);
            assert!(
                image
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel[3] > 0 && pixel[..3] != [255, 255, 255])
            );
        }
    }

    #[test]
    fn unicode_labels_do_not_create_a_renderer_disk_cache() {
        if std::env::var_os("GITTURTLE_MERMAID_CHILD").is_some() {
            let document = preview(
                "flowchart LR\nA[Révision Ελληνικά] --> B[完了]",
                Path::new("unicode.mmd"),
                800,
                || Ok(()),
            )
            .unwrap()
            .unwrap();
            assert!(
                document.diagrams[0].image.is_some(),
                "{:?}",
                document.diagrams[0].error
            );
            return;
        }
        let cache = tempfile::TempDir::new().unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "mermaid::tests::unicode_labels_do_not_create_a_renderer_disk_cache",
                "--exact",
            ])
            .env("GITTURTLE_MERMAID_CHILD", "1")
            .env("XDG_CACHE_HOME", cache.path())
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(std::fs::read_dir(cache.path()).unwrap().count(), 0);
    }

    #[test]
    fn markdown_fences_preserve_source_order_and_reject_unclosed_blocks() {
        let source = "# Read me\r\n```text\r\n```mermaid\r\n```\r\n~~~mermaid\r\nflowchart LR; A-->B\r\n~~~\r\n```mermaid\nflowchart TD; B-->C\n";
        let rendered = preview(source, Path::new("README.md"), 400, || Ok(()))
            .unwrap()
            .unwrap();
        assert_eq!(rendered.total, 2);
        assert_eq!(rendered.diagrams[0].source_line, 6);
        assert!(rendered.diagrams[0].image.is_some());
        assert!(
            rendered.diagrams[1]
                .error
                .as_deref()
                .unwrap()
                .contains("not closed")
        );
        assert!(
            preview("ordinary text", Path::new("a.md"), 400, || Ok(()))
                .unwrap()
                .is_none()
        );
        let five = "```mermaid\nflowchart LR; A-->B\n```\n".repeat(5);
        let rendered = preview(&five, Path::new("a.md"), 400, || Ok(()))
            .unwrap()
            .unwrap();
        assert_eq!(rendered.total, 5);
        assert_eq!(rendered.diagrams.len(), MAX_DIAGRAMS);
    }

    #[test]
    fn refuses_resources_unknown_types_and_oversized_input_without_losing_source() {
        for source in [
            "flowchart LR\nA-->B\nclick A \"https://example.invalid\"",
            "%%{init: {\"theme\": \"dark\"}}%%\nflowchart LR; A-->B",
            "flowchart LR\nA[\"<img src='file:///tmp/private'>\"]",
            "gantt\ntitle not enabled",
            "xychart-beta\nx-axis 0 --> 999999999",
            "not a diagram",
            &"x".repeat(MAX_DIAGRAM_BYTES + 1),
            &format!("flowchart LR\n{}", "A-->B\n".repeat(MAX_DIAGRAM_LINES)),
        ] {
            let rendered = preview(source, Path::new("a.mermaid"), 400, || Ok(()))
                .unwrap()
                .unwrap();
            assert!(rendered.diagrams[0].image.is_none(), "{source}");
            assert!(rendered.diagrams[0].error.is_some());
        }
        assert!(
            preview(
                "flowchart LR; A-->B",
                Path::new("a.mmd"),
                400,
                || anyhow::bail!("cancelled")
            )
            .is_err()
        );
    }

    #[test]
    fn bounds_fan_out_before_expansion_and_independent_math_nesting() {
        let fan_out = format!(
            "flowchart LR\n{} A --> {} B",
            "A & ".repeat(64),
            "B & ".repeat(64)
        );
        let disguised_nesting = format!(
            "flowchart LR\nA[\"$${}x{}$$\"]",
            "\\sqrt{)".repeat(40),
            "}".repeat(40)
        );
        for (source, reason) in [(&fan_out, "ampersand"), (&disguised_nesting, "nesting")] {
            let document = preview(source, Path::new("a.mmd"), 400, || Ok(()))
                .unwrap()
                .unwrap();
            assert!(document.diagrams[0].image.is_none());
            assert!(
                document.diagrams[0]
                    .error
                    .as_deref()
                    .unwrap()
                    .contains(reason)
            );
        }
        let ordinary = preview(
            "flowchart LR\nA & B --> C & D",
            Path::new("a.mmd"),
            400,
            || Ok(()),
        )
        .unwrap()
        .unwrap();
        assert!(
            ordinary.diagrams[0].image.is_some(),
            "{:?}",
            ordinary.diagrams[0].error
        );
    }
}
