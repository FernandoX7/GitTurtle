//! Structural work preflight for SVG reference and marker expansion.
//! Runs before usvg parsing, system font loading, or raster allocation.

use std::collections::HashMap;

use anyhow::{Result, ensure};

use super::{MAX_SVG_BYTES, MAX_SVG_DEPTH, MAX_SVG_NODES};

/// XML bounds alone do not bound the tree produced by `use`: a small group can
/// reference the previous group twice at each level. Count each expanded visit
/// and its attribute/text payload before usvg clones or parses that geometry.
/// Definitions count too because usvg expands their references during parsing.
pub(super) fn check_expansion(document: &roxmltree::Document<'_>) -> Result<()> {
    if document
        .descendants()
        .any(|node| svg_element(node, "marker"))
    {
        // Marker geometry is counted from presentation attributes below. A CSS
        // selector can attach markers to any path, including inside a marker.
        // Keep this finite preflight independent of a second CSS cascade. The
        // pinned renderer does not decode CSS Unicode escapes; refuse escapes
        // as well to keep the boundary explicit on later dependency updates.
        for node in document.descendants() {
            ensure!(
                !node
                    .attributes()
                    .any(|attribute| attribute.namespace().is_some()
                        && matches!(
                            attribute.name(),
                            "d" | "points" | "marker-start" | "marker-mid" | "marker-end"
                        )),
                "SVG namespaced geometry or marker attributes are not supported in bounded marker previews"
            );
            let styles = node
                .attributes()
                .filter(|attribute| attribute.name() == "style")
                .map(|attribute| attribute.value())
                .chain(node.text().filter(|_| node.tag_name().name() == "style"));
            for style in styles {
                let lower = style.to_ascii_lowercase();
                ensure!(
                    !lower.contains("url(") && !lower.contains("marker") && !style.contains('\\'),
                    "SVG markers with resource, marker, or escaped CSS are not supported in bounded previews; use marker presentation attributes"
                );
            }
        }
    }
    let mut ids = HashMap::new();
    for node in document.descendants() {
        if let Some(id) = node.attribute("id") {
            // Match usvg's first-ID rule, including malformed duplicate IDs.
            ids.entry(id).or_insert(node);
        }
    }
    let mut visits = 0usize;
    let mut payload = 0usize;
    let mut stack = Vec::new();
    check_svg_node(
        document.root(),
        &ids,
        &mut visits,
        &mut payload,
        &mut stack,
        [None; 3],
    )
}

fn check_svg_node<'a, 'input>(
    node: roxmltree::Node<'a, 'input>,
    ids: &HashMap<&str, roxmltree::Node<'a, 'input>>,
    visits: &mut usize,
    payload: &mut usize,
    stack: &mut Vec<roxmltree::NodeId>,
    inherited_markers: [Option<&'a str>; 3],
) -> Result<()> {
    ensure!(
        stack.len() < MAX_SVG_DEPTH && !stack.contains(&node.id()),
        "SVG expanded references contain a cycle or exceed the 128-level nesting limit"
    );
    *visits += 1;
    ensure!(
        *visits <= MAX_SVG_NODES,
        "SVG expanded references exceed the 10000-node limit"
    );
    let text_bytes = if node.is_text() {
        node.text().map_or(0, str::len)
    } else {
        0
    };
    *payload += text_bytes
        + node
            .attributes()
            .map(|attribute| attribute.value().len())
            .sum::<usize>();
    ensure!(
        *payload <= MAX_SVG_BYTES,
        "SVG expanded reference payload exceeds the 2 MiB limit"
    );
    stack.push(node.id());
    let markers = svg_markers(node, inherited_markers);
    for child in node.children() {
        check_svg_node(child, ids, visits, payload, stack, markers)?;
    }
    if svg_element(node, "use") {
        // SVG 2 href takes precedence over xlink:href, as in usvg. Do not let a
        // differently namespaced href or duplicate ID select another subtree.
        let href = node
            .attributes()
            .find(|attribute| attribute.name() == "href" && attribute.namespace().is_none())
            .or_else(|| {
                node.attributes().find(|attribute| {
                    attribute.name() == "href"
                        && attribute.namespace() == Some("http://www.w3.org/1999/xlink")
                })
            });
        if let Some(target) = href
            .and_then(|attribute| attribute.value().strip_prefix('#'))
            // The renderer's IRI parser accepts trailing ASCII whitespace.
            // Conservatively follow the fragment before the first space even
            // when the remaining suffix would make the renderer reject it.
            .and_then(|id| id.split(' ').next())
            .and_then(|id| ids.get(id))
        {
            check_svg_node(*target, ids, visits, payload, stack, markers)?;
        }
    }
    // Marker children are converted anew for every placement. Start/end have
    // one placement; mid markers can multiply with every geometry segment.
    let segments = svg_marker_segments(node);
    if segments > 0 {
        for (index, marker) in markers.into_iter().enumerate() {
            let Some(target) = marker
                .and_then(svg_marker_fragment)
                .and_then(|id| ids.get(id))
                .filter(|target| svg_element(**target, "marker"))
            else {
                continue;
            };
            // Markers inherit from their definition, not the referencing path.
            // In contrast, a use subtree above inherits from its use element.
            let mut inherited = [None; 3];
            let ancestors: Vec<_> = target.ancestors().skip(1).collect();
            for ancestor in ancestors.into_iter().rev() {
                inherited = svg_markers(ancestor, inherited);
            }
            let placements = if index == 1 { segments } else { 1 };
            for _ in 0..placements {
                check_svg_node(*target, ids, visits, payload, stack, inherited)?;
            }
        }
    }
    stack.pop();
    Ok(())
}

fn svg_markers<'a>(
    node: roxmltree::Node<'a, '_>,
    mut inherited: [Option<&'a str>; 3],
) -> [Option<&'a str>; 3] {
    for (index, name) in ["marker-start", "marker-mid", "marker-end"]
        .into_iter()
        .enumerate()
    {
        if let Some(value) = node.attribute(name)
            && value.trim_ascii() != "inherit"
        {
            inherited[index] = Some(value);
        }
    }
    inherited
}

fn svg_marker_fragment(value: &str) -> Option<&str> {
    // Conservative fragment extraction matching svgtypes' quoted/unquoted
    // FuncIRI forms. Invalid trailing syntax may overcount, never undercount.
    let value = value
        .trim_ascii_start()
        .strip_prefix("url(")?
        .trim_ascii_start();
    if let Some(quote @ ('\'' | '"')) = value.chars().next() {
        let value = value[1..].trim_ascii_start().strip_prefix('#')?;
        Some(value.split(quote).next()?.trim_end())
    } else {
        value.strip_prefix('#')?.split([' ', ')']).next()
    }
}

fn svg_marker_segments(node: roxmltree::Node<'_, '_>) -> usize {
    if !matches!(
        node.tag_name().namespace(),
        None | Some("http://www.w3.org/2000/svg")
    ) {
        return 0;
    }
    match node.tag_name().name() {
        // A source byte cannot specify more than one segment. Reserve four for
        // each byte to cover arc-to-cubic conversion conservatively; do not
        // parse geometry a second time just to estimate the marker work.
        "path" => node
            .attribute("d")
            .map_or(0, |value| value.len().saturating_mul(4)),
        "polyline" | "polygon" => node
            .attribute("points")
            .map_or(0, |value| value.len().saturating_mul(4)),
        "line" | "rect" | "circle" | "ellipse" => 16,
        _ => 0,
    }
}

fn svg_element(node: roxmltree::Node<'_, '_>, name: &str) -> bool {
    // usvg accepts omitted namespaces as well as ordinary SVG namespaces.
    node.is_element()
        && node.tag_name().name() == name
        && matches!(
            node.tag_name().namespace(),
            None | Some("http://www.w3.org/2000/svg")
        )
}
