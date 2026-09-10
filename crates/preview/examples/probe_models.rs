//! Captured-file corpus inspection: decoding and each requested raster outcome
//! are separate. Input paths are explicit CLI data; no model resources resolve.
use anyhow::{Context, Result, ensure};
use gitturtle_preview::{MAX_INPUT_BYTES, model3d};
use serde_json::json;
use std::{io::Read, path::PathBuf, time::Instant};

fn main() -> Result<()> {
    ensure!(
        !cfg!(debug_assertions),
        "Build probe_models in release mode"
    );
    let mut arguments = std::env::args_os().skip(1).peekable();
    let output = if arguments
        .peek()
        .is_some_and(|value| value == "--triangles-directory")
    {
        arguments.next();
        let path = PathBuf::from(
            arguments
                .next()
                .context("Missing triangle output directory")?,
        );
        std::fs::create_dir_all(&path)?;
        Some(path)
    } else {
        None
    };
    let paths: Vec<_> = arguments.map(PathBuf::from).collect();
    ensure!(
        !paths.is_empty(),
        "usage: probe_models [--triangles-directory DIR] MODEL..."
    );
    for (ordinal, path) in paths.iter().enumerate() {
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take((MAX_INPUT_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        let start = Instant::now();
        let geometry = model3d::decode_geometry(&bytes, &path.to_string_lossy(), || Ok(()));
        let decode_ms = start.elapsed().as_secs_f64() * 1000.;
        let mut row = json!({"name":path.file_name().unwrap_or_default().to_string_lossy(),
            "input_bytes": bytes.len(), "decode_ms":decode_ms});
        match geometry {
            Ok(geometry) => {
                row["decode_ok"] = true.into();
                row["triangles"] = geometry.scene.triangle_count().into();
                row["retained_bytes"] = geometry.scene.retained_bytes().into();
                row["minimum"] = json!(geometry.scene.bounds.minimum);
                row["maximum"] = json!(geometry.scene.bounds.maximum);
                let camera = model3d::ModelCamera::fit(geometry.scene.bounds);
                let mut renders = Vec::new();
                for (edge, wireframe) in [(360, false), (720, false), (720, true)] {
                    let start = Instant::now();
                    let rendered =
                        model3d::render_model(&geometry.scene, &camera, edge, wireframe, || Ok(()));
                    let milliseconds = start.elapsed().as_secs_f64() * 1000.;
                    renders.push(match rendered {
                        Ok(image) => json!({"edge":edge,"wireframe":wireframe,"ok":true,
                            "render_ms":milliseconds,"rgba_bytes":image.rgba.len()}),
                        Err(error) => json!({"edge":edge,"wireframe":wireframe,"ok":false,
                            "render_ms":milliseconds,"error":format!("{error:#}")}),
                    });
                }
                row["renders"] = renders.into();
                if let Some(output) = &output {
                    let name = format!("{ordinal:04}.triangles.bin");
                    let mut encoded = Vec::with_capacity(geometry.scene.triangle_count() * 72);
                    for triangle in geometry.scene.triangles() {
                        for point in triangle {
                            for coordinate in point {
                                encoded.extend_from_slice(&coordinate.to_le_bytes());
                            }
                        }
                    }
                    std::fs::write(output.join(&name), encoded)?;
                    row["triangle_file"] = name.into();
                }
            }
            Err(error) => {
                row["decode_ok"] = false.into();
                row["error"] = format!("{error:#}").into();
            }
        }
        println!("{row}");
    }
    Ok(())
}
