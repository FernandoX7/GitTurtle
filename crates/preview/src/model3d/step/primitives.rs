//! Analytic ISO STEP CSG primitives with deterministic bounded sampling. Curves
//! are evaluated from supplied scalar parameters, never guessed from point clouds.
use super::*;
use scene::{axis_placement, coordinate, real};

const AROUND: usize = 64;
const BANDS: usize = 32;

pub(super) fn supported(name: &str) -> bool {
    matches!(
        name,
        "SPHERE" | "RIGHT_CIRCULAR_CYLINDER" | "TORUS" | "BLOCK"
    )
}

fn positive(text: &str) -> Result<f64> {
    let value = real(text)?;
    ensure!(
        value > 1e-12,
        "STEP primitive dimensions must be positive and visible"
    );
    Ok(value)
}

pub(super) fn tessellate(
    record: &Record<'_>,
    records: &HashMap<usize, Record<'_>>,
    check: &impl Fn() -> Result<()>,
) -> Result<Vec<Triangle>> {
    let f = fields(record)?;
    let mut mesh = Vec::new();
    match record.name {
        "SPHERE" => {
            ensure!(f.len() == 3, "Invalid STEP sphere");
            let radius = positive(f[1])?;
            let center = coordinate(records, f[2], "CARTESIAN_POINT")?;
            let point = |i: usize, j: usize| {
                let theta = std::f64::consts::TAU * i as f64 / AROUND as f64;
                let phi =
                    std::f64::consts::PI * j as f64 / BANDS as f64 - std::f64::consts::FRAC_PI_2;
                let ring = if j == 0 || j == BANDS {
                    0.
                } else {
                    radius * phi.cos()
                };
                [
                    center[0] + ring * theta.cos(),
                    center[1] + ring * theta.sin(),
                    center[2] + radius * phi.sin(),
                ]
            };
            for j in 0..BANDS {
                for i in 0..AROUND {
                    check()?;
                    let next = (i + 1) % AROUND;
                    if j != 0 {
                        push_triangle(
                            &mut mesh,
                            [point(i, j), point(next, j), point(next, j + 1)],
                        )?;
                    }
                    if j != BANDS - 1 {
                        push_triangle(
                            &mut mesh,
                            [point(i, j), point(next, j + 1), point(i, j + 1)],
                        )?;
                    }
                }
            }
        }
        "RIGHT_CIRCULAR_CYLINDER" => {
            ensure!(f.len() == 4, "Invalid STEP cylinder");
            let matrix = axis_placement(records, f[1], true)?;
            let height = positive(f[2])?;
            let radius = positive(f[3])?;
            let point = |i: usize, z| {
                let theta = std::f64::consts::TAU * i as f64 / AROUND as f64;
                apply(matrix, [radius * theta.cos(), radius * theta.sin(), z])
            };
            let bottom = apply(matrix, [0., 0., 0.])?;
            let top = apply(matrix, [0., 0., height])?;
            for i in 0..AROUND {
                check()?;
                let next = (i + 1) % AROUND;
                let a = point(i, 0.)?;
                let b = point(next, 0.)?;
                let c = point(next, height)?;
                let d = point(i, height)?;
                for triangle in [[a, b, c], [a, c, d], [bottom, b, a], [top, d, c]] {
                    push_triangle(&mut mesh, triangle)?;
                }
            }
        }
        "TORUS" => {
            ensure!(f.len() == 4, "Invalid STEP torus");
            let matrix = axis_placement(records, f[1], true)?;
            let major = positive(f[2])?;
            let minor = positive(f[3])?;
            ensure!(
                major > minor,
                "STEP torus must have major radius larger than minor radius"
            );
            let point = |i: usize, j: usize| {
                let theta = std::f64::consts::TAU * i as f64 / AROUND as f64;
                let phi = std::f64::consts::TAU * j as f64 / BANDS as f64;
                let ring = major + minor * phi.cos();
                apply(
                    matrix,
                    [ring * theta.cos(), ring * theta.sin(), minor * phi.sin()],
                )
            };
            for j in 0..BANDS {
                for i in 0..AROUND {
                    check()?;
                    let ni = (i + 1) % AROUND;
                    let nj = (j + 1) % BANDS;
                    let a = point(i, j)?;
                    let b = point(ni, j)?;
                    let c = point(ni, nj)?;
                    let d = point(i, nj)?;
                    push_triangle(&mut mesh, [a, b, c])?;
                    push_triangle(&mut mesh, [a, c, d])?;
                }
            }
        }
        "BLOCK" => {
            ensure!(f.len() == 5, "Invalid STEP block");
            let matrix = axis_placement(records, f[1], false)?;
            let size = [positive(f[2])?, positive(f[3])?, positive(f[4])?];
            let mut points = [[0.; 3]; 8];
            for (i, point) in points.iter_mut().enumerate() {
                *point = apply(
                    matrix,
                    std::array::from_fn(|axis| if i & (1 << axis) != 0 { size[axis] } else { 0. }),
                )?;
            }
            for face in [
                [0, 2, 3, 1],
                [4, 5, 7, 6],
                [0, 1, 5, 4],
                [2, 6, 7, 3],
                [0, 4, 6, 2],
                [1, 3, 7, 5],
            ] {
                check()?;
                push_triangle(
                    &mut mesh,
                    [points[face[0]], points[face[1]], points[face[2]]],
                )?;
                push_triangle(
                    &mut mesh,
                    [points[face[0]], points[face[2]], points[face[3]]],
                )?;
            }
        }
        other => bail!("Unsupported STEP primitive {other}"),
    }
    Ok(mesh)
}
