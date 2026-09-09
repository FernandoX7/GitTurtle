//! Deliberately bounded STEP faceted-BREP subset. Unsupported CAD geometry is an
//! explicit error, never silently replaced with a bounding box or vertex cloud.

use super::*;

const MAX_STEP_BYTES: usize = 4 * 1024 * 1024;
const MAX_RECORDS: usize = 40_000;
const MAX_POLYGON_POINTS: usize = 256;
const MAX_POLYGON_WORK: usize = 4_000_000;

struct Record<'a> {
    name: &'a str,
    args: &'a str,
}

pub(super) fn faceted(bytes: &[u8], check: &impl Fn() -> Result<()>) -> Result<Vec<Triangle>> {
    ensure!(
        bytes.len() <= MAX_STEP_BYTES,
        "STEP exceeds the 4 MiB faceted preview limit"
    );
    let text = std::str::from_utf8(bytes).context("STEP preview requires a text exchange file")?;
    let text = without_comments(text)?;
    let mut records = HashMap::new();
    let mut in_data = false;
    let mut saw_end = false;
    for statement in split(&text, b';', MAX_RECORDS + 128)? {
        check()?;
        let statement = statement.trim();
        if statement == "DATA" || statement.starts_with("DATA(") {
            in_data = true;
            continue;
        }
        if statement == "ENDSEC" {
            in_data = false;
            continue;
        }
        if statement == "END-ISO-10303-21" {
            saw_end = true;
            continue;
        }
        if !in_data || statement.is_empty() {
            continue;
        }
        ensure!(
            records.len() < MAX_RECORDS && statement.len() <= 32 * 1024,
            "STEP entity exceeds preview limits"
        );
        let (id, value) = statement.split_once('=').context("Invalid STEP entity")?;
        let id = reference(id)?;
        let value = value.trim();
        let open = value.find('(').context("Invalid STEP entity parameters")?;
        let name = value[..open].trim();
        let args = value[open..]
            .strip_prefix('(')
            .and_then(|v| v.strip_suffix(')'))
            .context("Invalid STEP entity parameters")?;
        ensure!(
            !name.contains("TRANSFORM")
                && !matches!(
                    name,
                    "MAPPED_ITEM"
                        | "REPRESENTATION_MAP"
                        | "ADVANCED_BREP_SHAPE_REPRESENTATION"
                        | "MANIFOLD_SOLID_BREP"
                        | "BREP_WITH_VOIDS"
                        | "FACETED_BREP_WITH_VOIDS"
                ),
            "STEP curved solids or assembly transforms are not supported; open the captured source in a CAD application"
        );
        // Complex unit/context entities are harmless to this subset. Complex
        // geometry is rejected so it cannot hide an assembly placement.
        ensure!(
            !name.is_empty()
                || ![
                    "TRANSFORM",
                    "MAPPED_ITEM",
                    "REPRESENTATION_MAP",
                    "BREP",
                    "SHELL",
                    "FACE"
                ]
                .iter()
                .any(|token| args.contains(token)),
            "STEP complex geometry is not supported by the faceted preview"
        );
        ensure!(
            records.insert(id, Record { name, args }).is_none(),
            "Duplicate STEP entity ID"
        );
    }
    ensure!(
        text.trim_start().starts_with("ISO-10303-21;") && saw_end,
        "Invalid STEP exchange header or end marker"
    );
    let mut roots: Vec<_> = records
        .iter()
        .filter(|(_, record)| record.name == "FACETED_BREP")
        .collect();
    roots.sort_by_key(|(id, _)| **id);
    ensure!(
        !roots.is_empty(),
        "STEP native preview supports faceted B-rep solids only. Curved CAD surfaces require an external CAD application"
    );
    ensure!(
        roots.len() <= MAX_OBJECTS,
        "STEP exceeds the solid preview limit"
    );
    let mut mesh = Vec::new();
    let mut work = 0;
    for (_, solid) in roots {
        check()?;
        let solid_fields = fields(solid)?;
        ensure!(solid_fields.len() == 2, "Invalid STEP faceted solid");
        let shell = get(&records, solid_fields[1], "CLOSED_SHELL")?;
        let shell_fields = fields(shell)?;
        ensure!(shell_fields.len() == 2, "Invalid STEP closed shell");
        for face in aggregate(shell_fields[1], MAX_RECORDS)? {
            check()?;
            let face = records
                .get(&reference(face)?)
                .context("STEP references a missing face")?;
            ensure!(
                matches!(face.name, "FACE" | "FACE_SURFACE"),
                "STEP preview requires faceted polygon faces; curved surfaces are not rendered"
            );
            let face_fields = fields(face)?;
            if face.name == "FACE_SURFACE" {
                ensure!(face_fields.len() == 4, "Invalid STEP face surface");
                get(&records, face_fields[2], "PLANE")?;
                ensure!(
                    matches!(face_fields[3], ".T." | ".F."),
                    "Invalid STEP surface orientation"
                );
            } else {
                ensure!(face_fields.len() == 2, "Invalid STEP face");
            }
            let bounds = aggregate(face_fields[1], 1024)?;
            ensure!(
                bounds.len() == 1,
                "STEP faces with holes are not supported by the faceted preview"
            );
            let bound_id = reference(bounds[0])?;
            let bound = records
                .get(&bound_id)
                .context("Missing STEP face boundary")?;
            ensure!(
                matches!(bound.name, "FACE_OUTER_BOUND" | "FACE_BOUND"),
                "Unsupported STEP face boundary"
            );
            let bound_fields = fields(bound)?;
            ensure!(bound_fields.len() == 3, "Invalid STEP face boundary");
            let polygon = get(&records, bound_fields[1], "POLY_LOOP")?;
            let polygon_fields = fields(polygon)?;
            ensure!(polygon_fields.len() == 2, "Invalid STEP polygon");
            let mut points = Vec::new();
            for point in aggregate(polygon_fields[1], MAX_POLYGON_POINTS)? {
                let point = get(&records, point, "CARTESIAN_POINT")?;
                let point_fields = fields(point)?;
                ensure!(point_fields.len() == 2, "Invalid STEP point");
                let values = aggregate(point_fields[1], 3)?;
                ensure!(
                    values.len() == 3,
                    "STEP preview requires three-dimensional points"
                );
                let mut point = [0.; 3];
                for (target, value) in point.iter_mut().zip(values) {
                    *target = value.parse().context("Invalid STEP coordinate")?;
                }
                points.push(valid_point(point)?);
            }
            match bound_fields[2] {
                ".T." => {}
                ".F." => points.reverse(),
                _ => bail!("Invalid STEP boundary orientation"),
            }
            triangulate(&points, &mut mesh, &mut work, check)?;
        }
    }
    Ok(mesh)
}

fn reference(text: &str) -> Result<usize> {
    text.trim()
        .strip_prefix('#')
        .context("Expected STEP entity reference")?
        .parse()
        .context("Invalid STEP entity reference")
}

fn get<'a>(
    records: &'a HashMap<usize, Record<'a>>,
    id: &str,
    expected: &str,
) -> Result<&'a Record<'a>> {
    let record = records
        .get(&reference(id)?)
        .context("STEP references a missing entity")?;
    ensure!(
        record.name == expected,
        "Unsupported STEP geometry: expected {expected}, found {}",
        record.name
    );
    Ok(record)
}

fn fields<'a>(record: &Record<'a>) -> Result<Vec<&'a str>> {
    split(record.args, b',', MAX_RECORDS)
}

fn aggregate(text: &str, cap: usize) -> Result<Vec<&str>> {
    split(
        text.trim()
            .strip_prefix('(')
            .and_then(|v| v.strip_suffix(')'))
            .context("Invalid STEP aggregate")?,
        b',',
        cap,
    )
}

/// Split syntax delimiters, respecting quoted strings and nested aggregates.
fn split(text: &str, delimiter: u8, cap: usize) -> Result<Vec<&str>> {
    let mut values = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    let mut quote = false;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' => {
                if quote && bytes.get(i + 1) == Some(&b'\'') {
                    i += 2;
                    continue;
                }
                quote = !quote;
            }
            b'(' if !quote => {
                depth += 1;
                ensure!(depth <= 32, "STEP nesting exceeds preview limits");
            }
            b')' if !quote => {
                depth = depth.checked_sub(1).context("Unbalanced STEP parameters")?;
            }
            value if value == delimiter && !quote && depth == 0 => {
                ensure!(values.len() < cap, "STEP aggregate exceeds preview limits");
                values.push(text[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    ensure!(!quote && depth == 0, "Unterminated STEP parameters");
    if !text[start..].trim().is_empty() {
        ensure!(values.len() < cap, "STEP aggregate exceeds preview limits");
        values.push(text[start..].trim());
    }
    Ok(values)
}

fn without_comments(text: &str) -> Result<String> {
    let mut bytes = text.as_bytes().to_vec();
    let mut quote = false;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\'' {
            if quote && bytes.get(i + 1) == Some(&b'\'') {
                i += 2;
                continue;
            }
            quote = !quote;
        } else if !quote && bytes.get(i..i + 2) == Some(b"/*") {
            let start = i;
            i += 2;
            while i + 1 < bytes.len() && &bytes[i..i + 2] != b"*/" {
                i += 1;
            }
            ensure!(i + 1 < bytes.len(), "Unterminated STEP comment");
            i += 2;
            bytes[start..i].fill(b' ');
            continue;
        }
        i += 1;
    }
    String::from_utf8(bytes).context("Invalid STEP text")
}

pub(super) fn triangulate(
    points: &[Point],
    mesh: &mut Vec<Triangle>,
    work: &mut usize,
    check: &impl Fn() -> Result<()>,
) -> Result<()> {
    ensure!(
        points.len() >= 3,
        "STEP polygon has fewer than three points"
    );
    let mut normal = [0.; 3];
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        for j in 0..3 {
            normal[j] += (a[(j + 1) % 3] - b[(j + 1) % 3]) * (a[(j + 2) % 3] + b[(j + 2) % 3]);
        }
    }
    ensure!(dot(normal, normal) > 1e-24, "STEP polygon is degenerate");
    let normal = normalize(normal);
    let extent = points
        .iter()
        .map(|p| dot(subtract(*p, points[0]), subtract(*p, points[0])).sqrt())
        .fold(0., f64::max);
    ensure!(
        points
            .iter()
            .all(|p| dot(subtract(*p, points[0]), normal).abs() <= extent * 1e-7),
        "STEP polygon is not planar"
    );
    let axis = (0..3)
        .max_by(|&a, &b| normal[a].abs().total_cmp(&normal[b].abs()))
        .unwrap();
    let projected: Vec<Point> = points
        .iter()
        .map(|p| [p[(axis + 1) % 3], p[(axis + 2) % 3], 0.])
        .collect();
    let signed_area: f64 = (0..points.len())
        .map(|i| edge_value([0.; 3], projected[i], projected[(i + 1) % points.len()]))
        .sum();
    let direction = signed_area.signum();
    let mut remaining: Vec<usize> = (0..points.len()).collect();
    while remaining.len() > 3 {
        check()?;
        let mut found = false;
        for i in 0..remaining.len() {
            let a = remaining[(i + remaining.len() - 1) % remaining.len()];
            let b = remaining[i];
            let c = remaining[(i + 1) % remaining.len()];
            if edge_value(projected[a], projected[b], projected[c]) * direction <= 1e-12 {
                continue;
            }
            let mut inside = false;
            for &candidate in &remaining {
                *work += 1;
                ensure!(
                    *work <= MAX_POLYGON_WORK,
                    "STEP polygon triangulation exceeds preview work limits"
                );
                if [a, b, c].contains(&candidate) {
                    continue;
                }
                let p = projected[candidate];
                if edge_value(projected[a], projected[b], p) * direction >= 0.
                    && edge_value(projected[b], projected[c], p) * direction >= 0.
                    && edge_value(projected[c], projected[a], p) * direction >= 0.
                {
                    inside = true;
                    break;
                }
            }
            if !inside {
                push_triangle(mesh, [points[a], points[b], points[c]])?;
                remaining.remove(i);
                found = true;
                break;
            }
        }
        ensure!(
            found,
            "STEP polygon cannot be triangulated by the faceted preview"
        );
    }
    push_triangle(
        mesh,
        [
            points[remaining[0]],
            points[remaining[1]],
            points[remaining[2]],
        ],
    )
}
