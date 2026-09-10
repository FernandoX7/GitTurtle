//! Captured-byte appearance/deformation reference export and corpus observations.
//! Keep private inputs and output directories outside the checkout. This is a
//! release-only validation example, not a production resource-loading API.
use anyhow::{Context, Result, ensure};
use gitturtle_preview::{MAX_INPUT_BYTES, model3d};
use serde_json::{Value, json};
use std::{io::Read, path::Path, path::PathBuf, time::Instant};

fn classify(error: &str) -> &'static str {
    let lower = error.to_lowercase();
    if lower.contains("no triangles")
        || lower.contains("no triangle")
        || lower.contains("no renderable triangle")
    {
        "empty"
    } else if lower.contains("unsupported")
        || lower.contains("not supported")
        || lower.contains("exceeds")
        || lower.contains("limit")
        || lower.contains("budget")
    {
        "unsupported"
    } else {
        "malformed"
    }
}

fn capture(
    scene: &model3d::ModelScene,
    ordinal: usize,
    clip: Option<usize>,
    time: Option<f64>,
    output: &Path,
    corpus: bool,
) -> Result<Value> {
    let mut row = json!({"clip":clip,"time":time,"triangles":scene.triangle_count(),
        "minimum":scene.bounds.minimum,"maximum":scene.bounds.maximum,
        "retained_bytes":scene.retained_bytes()});
    if !corpus {
        let label = match (clip, time) {
            (Some(clip), Some(time)) => format!("clip{clip}-t{time}"),
            _ => "default".into(),
        };
        let name = format!("{ordinal:04}-{label}.triangles.bin");
        let mut encoded = Vec::with_capacity(scene.triangle_count() * 72);
        for triangle in scene.triangles() {
            for point in triangle {
                for coordinate in point {
                    encoded.extend_from_slice(&coordinate.to_le_bytes());
                }
            }
        }
        std::fs::write(output.join(&name), encoded)?;
        row["triangle_file"] = name.into();
    }
    let mut camera = model3d::ModelCamera::fit(scene.bounds);
    camera.set_view(model3d::ModelStandardView::Front);
    let mut renders = Vec::new();
    let variants: &[(u32, bool)] = if corpus {
        &[(360, false), (720, false), (720, true)]
    } else {
        &[(720, false)]
    };
    for &(edge, wireframe) in variants {
        let start = Instant::now();
        let image = model3d::render_model(scene, &camera, edge, wireframe, || Ok(()));
        let milliseconds = start.elapsed().as_secs_f64() * 1000.;
        match image {
            Ok(image) => {
                if !corpus {
                    let pixels = [
                        [256usize, 256usize],
                        [464, 256],
                        [256, 464],
                        [464, 464],
                        [320, 300],
                        [400, 350],
                    ];
                    row["appearance_samples"] = pixels
                        .iter()
                        .map(|&[x, y]| {
                            let offset = (y * image.width as usize + x) * 4;
                            json!({"pixel":[x,y],"rgba":&image.rgba[offset..offset+4]})
                        })
                        .collect::<Vec<_>>()
                        .into();
                }
                renders.push(
                    json!({"edge":edge,"wireframe":wireframe,"ok":true,"render_ms":milliseconds}),
                );
            }
            Err(error) => renders.push(json!({"edge":edge,"wireframe":wireframe,"ok":false,
                "render_ms":milliseconds,"error":format!("{error:#}")})),
        }
    }
    row["renders"] = renders.into();
    Ok(row)
}

fn main() -> Result<()> {
    ensure!(
        !cfg!(debug_assertions),
        "Build probe_glb_workflow in release mode"
    );
    let mut arguments = std::env::args_os().skip(1).peekable();
    let corpus = arguments.peek().is_some_and(|arg| arg == "--corpus");
    if corpus {
        arguments.next();
    }
    let output = PathBuf::from(
        arguments
            .next()
            .context("usage: probe_glb_workflow [--corpus] OUTPUT_DIRECTORY MODEL...")?,
    );
    let sources: Vec<PathBuf> = arguments.map(PathBuf::from).collect();
    ensure!(!sources.is_empty(), "Supply at least one captured model");
    std::fs::create_dir_all(&output)?;
    for (ordinal, source) in sources.iter().enumerate() {
        let mut bytes = Vec::new();
        std::fs::File::open(source)?
            .take((MAX_INPUT_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        let start = Instant::now();
        let geometry = model3d::decode_geometry(&bytes, &source.to_string_lossy(), || Ok(()));
        let mut row = json!({"ordinal":ordinal,"name":source.file_name().unwrap_or_default().to_string_lossy(),
            "input_bytes":bytes.len(),"decode_ms":start.elapsed().as_secs_f64()*1000.,
            "sample_policy":if corpus {"authored default plus each clip midpoint"} else {"authored default plus selected timestamps and each clip quartiles/end"}});
        match geometry {
            Err(error) => {
                let error = format!("{error:#}");
                row["ok"] = false.into();
                row["classification"] = classify(&error).into();
                row["error"] = error.into();
            }
            Ok(geometry) => {
                row["ok"] = true.into();
                row["classification"] = "supported".into();
                row["details"] = json!(geometry.details);
                let clips = geometry.scene.animation_clips();
                row["clips"] = clips
                    .iter()
                    .map(|clip| json!({"name":clip.name,"duration":clip.duration_seconds}))
                    .collect::<Vec<_>>()
                    .into();
                let mut samples = vec![capture(
                    &geometry.scene,
                    ordinal,
                    None,
                    None,
                    &output,
                    corpus,
                )?];
                for (index, clip) in clips.iter().enumerate() {
                    let duration = clip.duration_seconds;
                    let mut times = if corpus {
                        vec![duration * 0.5]
                    } else {
                        vec![
                            0.,
                            0.25,
                            0.5,
                            1.,
                            1.5,
                            2.,
                            duration * 0.25,
                            duration * 0.5,
                            duration * 0.75,
                            duration,
                        ]
                    };
                    times.iter_mut().for_each(|time| *time = time.min(duration));
                    times.sort_by(f64::total_cmp);
                    times.dedup();
                    for time in times {
                        let start = Instant::now();
                        let evaluated = geometry.scene.evaluate_animation(index, time, &|| Ok(()));
                        let milliseconds = start.elapsed().as_secs_f64() * 1000.;
                        match evaluated {
                            Ok(scene) => {
                                let mut sample = capture(
                                    &scene,
                                    ordinal,
                                    Some(index),
                                    Some(time),
                                    &output,
                                    corpus,
                                )?;
                                sample["evaluate_ms"] = milliseconds.into();
                                samples.push(sample);
                            }
                            Err(error) => samples
                                .push(json!({"clip":index,"time":time,"evaluate_ms":milliseconds,
                                "error":format!("{error:#}")})),
                        }
                    }
                }
                row["samples"] = samples.into();
            }
        }
        println!("{row}");
    }
    Ok(())
}
