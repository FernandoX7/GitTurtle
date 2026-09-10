//! STEP representation roots, explicit placement and unit handling. This is a
//! finite preview contract, not an AP203/AP214/AP242 conformance implementation.
use super::*;
use std::collections::HashSet;

pub(in crate::model3d) struct StepGeometry {
    pub mesh: Vec<Triangle>,
    pub units: ModelUnits,
    pub details: Vec<String>,
}

fn representation(name: &str) -> bool {
    matches!(
        name,
        "SHAPE_REPRESENTATION" | "FACETED_BREP_SHAPE_REPRESENTATION" | "CSG_SHAPE_REPRESENTATION"
    )
}

fn solid(name: &str) -> bool {
    matches!(name, "FACETED_BREP" | "CSG_SOLID")
}

pub(super) fn validate_record(record: &Record<'_>) -> Result<()> {
    let name = record.name;
    ensure!(
        (!name.contains("TRANSFORM") || name == "CARTESIAN_TRANSFORMATION_OPERATOR_3D")
            && !matches!(
                name,
                "ADVANCED_BREP_SHAPE_REPRESENTATION"
                    | "MANIFOLD_SOLID_BREP"
                    | "BREP_WITH_VOIDS"
                    | "FACETED_BREP_WITH_VOIDS"
                    | "BOOLEAN_RESULT"
                    | "BOOLEAN_CLIPPING_RESULT"
                    | "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION"
                    | "SHAPE_REPRESENTATION_RELATIONSHIP"
                    | "SOLID_REPLICA"
            ),
        "Unsupported STEP entity {name}: native CAD preview supports faceted solids, sphere/cylinder/torus/block CSG primitives and mapped instances; trimmed curved B-rep and product-relationship assemblies require a CAD application"
    );
    if name.is_empty() {
        for component in components(record.args)? {
            ensure!(
                matches!(
                    component.name,
                    "LENGTH_UNIT"
                        | "PLANE_ANGLE_UNIT"
                        | "SOLID_ANGLE_UNIT"
                        | "NAMED_UNIT"
                        | "SI_UNIT"
                        | "CONVERSION_BASED_UNIT"
                        | "GEOMETRIC_REPRESENTATION_CONTEXT"
                        | "GLOBAL_UNIT_ASSIGNED_CONTEXT"
                        | "GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT"
                        | "REPRESENTATION_CONTEXT"
                ),
                "Unsupported STEP complex entity {}; complex CAD geometry is not rendered",
                component.name
            );
        }
    }
    Ok(())
}

pub(super) fn prepare(
    records: &HashMap<usize, Record<'_>>,
    check: &impl Fn() -> Result<()>,
) -> Result<StepGeometry> {
    validate_graph(records, check)?;
    let mut mapped_reps = HashSet::new();
    let mut declared_units = Vec::new();
    let mut roots = Vec::new();
    for (&id, record) in records {
        check()?;
        if record.name == "REPRESENTATION_MAP" {
            let f = fields(record)?;
            ensure!(f.len() == 2, "Invalid STEP representation map");
            mapped_reps.insert(reference(f[1])?);
        }
        if representation(record.name) {
            let f = fields(record)?;
            ensure!(f.len() == 3, "Invalid STEP shape representation");
            declared_units.push(context_units(records, f[2])?);
            roots.push(id);
        }
    }
    // A mapped representation is rendered only through an instance. Overlaying
    // its unplaced definition would show fictitious geometry at the origin.
    roots.retain(|id| !mapped_reps.contains(id));
    if declared_units.is_empty() {
        ensure!(
            mapped_reps.is_empty() && !records.values().any(|r| r.name == "MAPPED_ITEM"),
            "STEP mapped items require a root shape representation"
        );
        roots.extend(
            records
                .iter()
                .filter_map(|(&id, r)| solid(r.name).then_some(id)),
        );
    }
    roots.sort_unstable();
    ensure!(
        !roots.is_empty(),
        "STEP has no supported root representation; native preview supports faceted solids and sphere/cylinder/torus/block CSG primitives, not general curved B-rep"
    );
    ensure!(
        roots.len() <= MAX_OBJECTS,
        "STEP exceeds the root representation limit"
    );
    let unit = declared_units.first().copied().flatten();
    ensure!(
        declared_units.iter().all(|v| match (unit, *v) {
            (None, None) => true,
            (Some(a), Some(b)) => (a - b).abs() <= a.abs() * 1e-12,
            _ => false,
        }),
        "STEP representations use mixed or inconsistent length units; this assembly cannot be compared at a reliable scale"
    );
    let mut output = Vec::new();
    let mut state = Expansion {
        records,
        stack: Vec::new(),
        instances: 0,
        polygon_work: 0,
        output: &mut output,
        check,
        curved: false,
    };
    for root in roots {
        state.expand(root, IDENTITY)?;
    }
    let curved = state.curved;
    let instances = state.instances;
    if let Some(factor) = unit {
        for triangle in &mut output {
            check()?;
            for point in triangle {
                *point = valid_point(point.map(|v| v * factor))?;
            }
        }
    }
    let mut details = vec![
        "STEP subset: faceted B-rep and analytic CSG sphere, cylinder, torus and block; mapped representation placements are applied. General trimmed curved B-rep, booleans, holes and product-relationship assemblies are unsupported.".into(),
        if let Some(factor) = unit { format!("Declared length units converted to millimeters (1 source unit = {factor:.8} mm)") }
            else { "Length units are unknown; source coordinates are preserved and physical scale is not established".into() },
        format!("{instances} bounded representation/solid visits; colors, textures, PMI and manufacturing tolerances are not rendered"),
    ];
    if curved {
        details.push("Analytic primitives use 64 segments per turn (sphere 32 latitude bands, torus 32 tube segments); this is a visual approximation, not manufacturing geometry".into());
    }
    Ok(StepGeometry {
        mesh: output,
        units: if unit.is_some() {
            ModelUnits::Millimeters
        } else {
            ModelUnits::Unknown
        },
        details,
    })
}

fn validate_graph(
    records: &HashMap<usize, Record<'_>>,
    check: &impl Fn() -> Result<()>,
) -> Result<()> {
    fn visit(
        id: usize,
        records: &HashMap<usize, Record<'_>>,
        done: &mut HashSet<usize>,
        stack: &mut Vec<usize>,
        check: &impl Fn() -> Result<()>,
    ) -> Result<()> {
        check()?;
        ensure!(
            !stack.contains(&id) && stack.len() < 32,
            "STEP mapped representation contains a cycle or exceeds the 32-level limit"
        );
        if done.contains(&id) {
            return Ok(());
        }
        let record = records
            .get(&id)
            .context("STEP references a missing representation item")?;
        stack.push(id);
        if representation(record.name) {
            let f = fields(record)?;
            ensure!(f.len() == 3, "Invalid STEP shape representation");
            for child in aggregate(f[1], MAX_OBJECTS)? {
                visit(reference(child)?, records, done, stack, check)?;
            }
        } else if record.name == "MAPPED_ITEM" {
            let f = fields(record)?;
            ensure!(f.len() == 3, "Invalid STEP mapped item");
            let map = fields(get(records, f[1], "REPRESENTATION_MAP")?)?;
            ensure!(map.len() == 2, "Invalid STEP representation map");
            axis_placement(records, map[0], false)?;
            mapping_target(records, f[2])?;
            visit(reference(map[1])?, records, done, stack, check)?;
        }
        stack.pop();
        done.insert(id);
        Ok(())
    }
    let mut done = HashSet::new();
    for (&id, record) in records {
        if representation(record.name) || record.name == "MAPPED_ITEM" {
            visit(id, records, &mut done, &mut Vec::new(), check)?;
        }
    }
    Ok(())
}

struct Expansion<'a, 'source, F> {
    records: &'a HashMap<usize, Record<'source>>,
    stack: Vec<usize>,
    instances: usize,
    polygon_work: usize,
    output: &'a mut Vec<Triangle>,
    check: &'a F,
    curved: bool,
}

impl<F: Fn() -> Result<()>> Expansion<'_, '_, F> {
    fn expand(&mut self, id: usize, placement: Transform) -> Result<()> {
        (self.check)()?;
        ensure!(
            self.stack.len() < 32 && !self.stack.contains(&id),
            "STEP mapped representation contains a cycle or exceeds the 32-level limit"
        );
        self.instances += 1;
        ensure!(
            self.instances <= MAX_OBJECTS,
            "STEP exceeds the 4,096 expanded instance limit"
        );
        let record = self
            .records
            .get(&id)
            .context("STEP references a missing representation item")?;
        let f = fields(record)?;
        self.stack.push(id);
        match record.name {
            name if representation(name) => {
                ensure!(f.len() == 3, "Invalid STEP shape representation");
                for child in aggregate(f[1], MAX_OBJECTS)? {
                    let child = reference(child)?;
                    let r = self
                        .records
                        .get(&child)
                        .context("STEP references a missing shape item")?;
                    if r.name != "AXIS2_PLACEMENT_3D" {
                        self.expand(child, placement)?;
                    }
                }
            }
            "MAPPED_ITEM" => {
                ensure!(f.len() == 3, "Invalid STEP mapped item");
                let map = fields(get(self.records, f[1], "REPRESENTATION_MAP")?)?;
                ensure!(map.len() == 2, "Invalid STEP representation map");
                let origin = axis_placement(self.records, map[0], false)?;
                let target = mapping_target(self.records, f[2])?;
                let local = compose(target, inverse_rigid(origin));
                self.expand(reference(map[1])?, compose(placement, local))?;
            }
            "CSG_SOLID" => {
                ensure!(f.len() == 2, "Invalid STEP CSG solid");
                let primitive = self
                    .records
                    .get(&reference(f[1])?)
                    .context("STEP references a missing CSG primitive")?;
                ensure!(
                    primitives::supported(primitive.name),
                    "STEP CSG supports single sphere/cylinder/torus/block primitives only; boolean trees are not rendered"
                );
                self.curved |= primitive.name != "BLOCK";
                let mesh = primitives::tessellate(primitive, self.records, self.check)?;
                self.append(mesh, placement)?;
            }
            "FACETED_BREP" => {
                let mut mesh = Vec::new();
                faceted_solid(
                    record,
                    self.records,
                    &mut mesh,
                    &mut self.polygon_work,
                    self.check,
                )?;
                self.append(mesh, placement)?;
            }
            other => bail!(
                "Unsupported STEP representation item {other}; no partial CAD geometry is substituted"
            ),
        }
        self.stack.pop();
        Ok(())
    }

    fn append(&mut self, triangles: Vec<Triangle>, matrix: Transform) -> Result<()> {
        ensure!(
            triangles.len() <= MAX_MODEL_TRIANGLES.saturating_sub(self.output.len()),
            "STEP exceeds the 100,000 expanded triangle limit"
        );
        for triangle in triangles {
            (self.check)()?;
            let mut out = [[0.; 3]; 3];
            for (to, from) in out.iter_mut().zip(triangle) {
                *to = apply(matrix, from)?;
            }
            push_triangle(self.output, out)?;
        }
        Ok(())
    }
}

pub(super) fn coordinate(
    records: &HashMap<usize, Record<'_>>,
    id: &str,
    name: &str,
) -> Result<Point> {
    let f = fields(get(records, id, name)?)?;
    ensure!(f.len() == 2, "Invalid STEP {name}");
    let values = aggregate(f[1], 3)?;
    ensure!(
        values.len() == 3,
        "STEP preview requires three-dimensional {name}"
    );
    valid_point([real(values[0])?, real(values[1])?, real(values[2])?])
}

pub(super) fn real(text: &str) -> Result<f64> {
    let value: f64 = text.parse().context("Invalid STEP numeric parameter")?;
    ensure!(
        value.is_finite() && value.abs() <= 1e12,
        "Invalid or unreasonable STEP numeric parameter"
    );
    Ok(value)
}

fn direction(records: &HashMap<usize, Record<'_>>, id: &str, fallback: Point) -> Result<Point> {
    if id == "$" {
        return Ok(fallback);
    }
    let value = coordinate(records, id, "DIRECTION")?;
    ensure!(
        dot(value, value) > 1e-24,
        "STEP has a zero placement direction"
    );
    Ok(normalize(value))
}

pub(super) fn axis_placement(
    records: &HashMap<usize, Record<'_>>,
    id: &str,
    axis1: bool,
) -> Result<Transform> {
    let name = if axis1 {
        "AXIS1_PLACEMENT"
    } else {
        "AXIS2_PLACEMENT_3D"
    };
    let f = fields(get(records, id, name)?)?;
    ensure!(
        f.len() == if axis1 { 3 } else { 4 },
        "Invalid STEP axis placement"
    );
    let origin = coordinate(records, f[1], "CARTESIAN_POINT")?;
    let z = direction(records, f[2], [0., 0., 1.])?;
    let fallback = if z[0].abs() < 0.9 {
        [1., 0., 0.]
    } else {
        [0., 1., 0.]
    };
    let x = if axis1 {
        fallback
    } else {
        direction(records, f[3], fallback)?
    };
    let x = subtract(x, z.map(|v| v * dot(x, z)));
    ensure!(dot(x, x) > 1e-24, "STEP placement axes are parallel");
    let x = normalize(x);
    let y = cross(z, x);
    Ok([
        x[0], x[1], x[2], y[0], y[1], y[2], z[0], z[1], z[2], origin[0], origin[1], origin[2],
    ])
}

fn mapping_target(records: &HashMap<usize, Record<'_>>, id: &str) -> Result<Transform> {
    let f = fields(get(records, id, "CARTESIAN_TRANSFORMATION_OPERATOR_3D")?)?;
    // Older application protocols have inherited transformation name/description
    // in addition to the representation-item name; the geometry tail is equal.
    ensure!(
        matches!(f.len(), 6 | 8),
        "Invalid STEP Cartesian transformation"
    );
    let f = &f[f.len() - 5..];
    let x = direction(records, f[0], [1., 0., 0.])?;
    let y = direction(records, f[1], [0., 1., 0.])?;
    let origin = coordinate(records, f[2], "CARTESIAN_POINT")?;
    let scale = if f[3] == "$" { 1. } else { real(f[3])? };
    let z = direction(records, f[4], cross(x, y))?;
    ensure!(
        scale > 0.
            && dot(x, y).abs() < 1e-8
            && dot(x, z).abs() < 1e-8
            && dot(y, z).abs() < 1e-8
            && dot(z, z) > 0.99,
        "STEP preview requires a positive uniform scale and orthogonal mapping axes"
    );
    Ok([
        x[0] * scale,
        x[1] * scale,
        x[2] * scale,
        y[0] * scale,
        y[1] * scale,
        y[2] * scale,
        z[0] * scale,
        z[1] * scale,
        z[2] * scale,
        origin[0],
        origin[1],
        origin[2],
    ])
}

fn compose(outer: Transform, inner: Transform) -> Transform {
    let mut matrix = [0.; 12];
    for column in 0..3 {
        for row in 0..3 {
            matrix[column * 3 + row] = (0..3)
                .map(|k| outer[k * 3 + row] * inner[column * 3 + k])
                .sum();
        }
    }
    for row in 0..3 {
        matrix[9 + row] = outer[9 + row]
            + (0..3)
                .map(|k| outer[k * 3 + row] * inner[9 + k])
                .sum::<f64>();
    }
    matrix
}

fn inverse_rigid(matrix: Transform) -> Transform {
    let mut inverse = [0.; 12];
    for row in 0..3 {
        for column in 0..3 {
            inverse[column * 3 + row] = matrix[row * 3 + column];
        }
    }
    for row in 0..3 {
        inverse[9 + row] = -(0..3)
            .map(|k| inverse[k * 3 + row] * matrix[9 + k])
            .sum::<f64>();
    }
    inverse
}

/// Complex STEP records concatenate component records without commas.
fn components(mut text: &str) -> Result<Vec<Record<'_>>> {
    let mut records = Vec::new();
    while !text.trim().is_empty() {
        text = text.trim_start();
        ensure!(
            records.len() < 16,
            "STEP complex entity has too many components"
        );
        let open = text.find('(').context("Invalid STEP complex entity")?;
        let mut depth = 0usize;
        let mut quote = false;
        let mut end = None;
        for (i, byte) in text.as_bytes().iter().copied().enumerate().skip(open) {
            match byte {
                b'\'' => quote = !quote,
                b'(' if !quote => depth += 1,
                b')' if !quote => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let end = end.context("Unterminated STEP complex entity")?;
        records.push(Record {
            name: text[..open].trim(),
            args: &text[open + 1..end],
        });
        text = &text[end + 1..];
    }
    Ok(records)
}

fn context_units(records: &HashMap<usize, Record<'_>>, id: &str) -> Result<Option<f64>> {
    let record = records
        .get(&reference(id)?)
        .context("STEP references a missing representation context")?;
    let parts = if record.name.is_empty() {
        components(record.args)?
    } else {
        vec![Record {
            name: record.name,
            args: record.args,
        }]
    };
    let mut length = None;
    for part in parts {
        if part.name == "GEOMETRIC_REPRESENTATION_CONTEXT" {
            ensure!(
                real(part.args)? == 3.,
                "STEP preview requires a three-dimensional context"
            );
        }
        if part.name == "GLOBAL_UNIT_ASSIGNED_CONTEXT" {
            for unit in aggregate(part.args, 8)? {
                if let Some(value) = unit_scale(records, unit, &mut Vec::new())? {
                    ensure!(
                        length.is_none(),
                        "STEP context contains multiple length units"
                    );
                    length = Some(value);
                }
            }
        }
    }
    Ok(length)
}

fn unit_scale(
    records: &HashMap<usize, Record<'_>>,
    id: &str,
    stack: &mut Vec<usize>,
) -> Result<Option<f64>> {
    let id = reference(id)?;
    ensure!(
        stack.len() < 8 && !stack.contains(&id),
        "STEP unit conversion contains a cycle or exceeds limits"
    );
    stack.push(id);
    let record = records.get(&id).context("STEP references a missing unit")?;
    let parts = if record.name.is_empty() {
        components(record.args)?
    } else {
        vec![Record {
            name: record.name,
            args: record.args,
        }]
    };
    if !parts.iter().any(|p| p.name == "LENGTH_UNIT") {
        stack.pop();
        return Ok(None);
    }
    let unit = parts
        .iter()
        .find(|p| matches!(p.name, "SI_UNIT" | "CONVERSION_BASED_UNIT"))
        .context("Unsupported STEP length unit")?;
    let f = fields(unit)?;
    ensure!(f.len() == 2, "Invalid STEP length unit");
    let factor = if unit.name == "SI_UNIT" {
        ensure!(f[1] == ".METRE.", "Unsupported STEP SI length unit");
        match f[0] {
            "$" => 1000.,
            ".MILLI." => 1.,
            ".CENTI." => 10.,
            ".DECI." => 100.,
            ".MICRO." => 0.001,
            ".NANO." => 0.000001,
            ".KILO." => 1_000_000.,
            _ => bail!("Unsupported STEP SI length prefix"),
        }
    } else {
        let measure = records
            .get(&reference(f[1])?)
            .context("STEP references a missing unit measure")?;
        ensure!(
            matches!(
                measure.name,
                "LENGTH_MEASURE_WITH_UNIT" | "MEASURE_WITH_UNIT"
            ),
            "Unsupported STEP conversion measure"
        );
        let f = fields(measure)?;
        ensure!(f.len() == 2, "Invalid STEP conversion measure");
        let value = f[0]
            .strip_prefix("LENGTH_MEASURE(")
            .and_then(|v| v.strip_suffix(')'))
            .context("STEP conversion must contain a length measure")?;
        real(value)?
            * unit_scale(records, f[1], stack)?
                .context("STEP conversion references a non-length unit")?
    };
    ensure!(
        factor.is_finite() && (1e-9..=1e12).contains(&factor),
        "STEP length conversion exceeds preview limits"
    );
    stack.pop();
    Ok(Some(factor))
}
