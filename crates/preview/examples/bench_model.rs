//! Release-only supplied-byte geometry and retained-camera raster measurements.
//! File I/O, image destruction, native scheduling, GPUI and GPU upload are excluded.
use anyhow::{Context, Result, ensure};
use gitturtle_preview::{MAX_INPUT_BYTES, model3d};
use std::{hint::black_box, io::Read, path::PathBuf, time::Instant};

const USAGE: &str = "usage: bench_model MODEL [SAMPLES=30] [WARMUPS=3]";

struct Sample {
    decode_ms: f64,
    render_ms: [f64; 3],
    triangles: usize,
    scene_bytes: usize,
}

fn sample(bytes: &[u8], name: &str) -> Result<Sample> {
    let start = Instant::now();
    let geometry = black_box(model3d::decode_geometry(black_box(bytes), name, || Ok(()))?);
    let decode_ms = start.elapsed().as_secs_f64() * 1000.;
    let camera = model3d::ModelCamera::fit(geometry.scene.bounds);
    let mut render_ms = [0.; 3];
    for (index, (edge, wireframe)) in [(360, false), (720, false), (720, true)]
        .into_iter()
        .enumerate()
    {
        let start = Instant::now();
        let image = black_box(model3d::render_model(
            black_box(&geometry.scene),
            black_box(&camera),
            edge,
            wireframe,
            || Ok(()),
        )?);
        render_ms[index] = start.elapsed().as_secs_f64() * 1000.;
        ensure!(image.rgba.len() == edge as usize * edge as usize * 4);
        // Exclude output destruction; retain only one output frame at a time.
        drop(image);
    }
    Ok(Sample {
        decode_ms,
        render_ms,
        triangles: geometry.scene.triangle_count(),
        scene_bytes: geometry.scene.retained_bytes(),
    })
}

fn print_sample(phase: &str, index: usize, value: &Sample) {
    println!(
        "{phase}\t{index}\t{:.6}\t{:.6}\t{:.6}\t{:.6}\t{}\t{}",
        value.decode_ms,
        value.render_ms[0],
        value.render_ms[1],
        value.render_ms[2],
        value.triangles,
        value.scene_bytes
    );
}

fn main() -> Result<()> {
    ensure!(
        !cfg!(debug_assertions),
        "Use cargo run --release --locked -p gitturtle-preview --example bench_model -- MODEL"
    );
    let mut arguments = std::env::args_os().skip(1);
    let path = PathBuf::from(arguments.next().context(USAGE)?);
    let samples: usize = arguments
        .next()
        .map(|argument| argument.to_string_lossy().parse())
        .transpose()?
        .unwrap_or(30);
    let warmups: usize = arguments
        .next()
        .map(|argument| argument.to_string_lossy().parse())
        .transpose()?
        .unwrap_or(3);
    ensure!(arguments.next().is_none(), USAGE);
    ensure!(
        (1..=10_000).contains(&samples) && warmups <= 1_000,
        "Use 1–10,000 samples and 0–1,000 warmups"
    );
    let mut bytes = Vec::new();
    std::fs::File::open(&path)?
        .take((MAX_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= MAX_INPUT_BYTES, "Input exceeds 32 MiB");
    let name = path.to_string_lossy();
    println!("# model={:?}", path);
    println!("# executable={:?}", std::env::current_exe()?);
    println!("# profile=release; input_bytes={}", bytes.len());
    println!(
        "# cache=input bytes resident before timing; no application cache; fresh decode per row; render uses that row's retained scene"
    );
    println!(
        "# boundary=decode_geometry and render_model CPU calls; excludes file I/O, camera fit, output destruction, native scheduling, GPUI, GPU and OS presentation"
    );
    println!(
        "# raster=fit isometric; each render allocates a fresh frame; 360 solid, 720 solid, 720 all-edge wireframe"
    );
    println!(
        "phase\tsample\tdecode_ms\trender_360_solid_ms\trender_720_solid_ms\trender_720_wireframe_ms\ttriangles\tscene_retained_bytes"
    );
    print_sample("first", 0, &sample(&bytes, &name)?);
    for index in 0..warmups {
        print_sample("warmup", index, &sample(&bytes, &name)?);
    }
    let mut results = Vec::with_capacity(samples);
    for index in 0..samples {
        let result = sample(&bytes, &name)?;
        print_sample("measured", index, &result);
        results.push(result);
    }
    for (metric, index) in [
        ("decode_ms", None),
        ("render_360_solid_ms", Some(0)),
        ("render_720_solid_ms", Some(1)),
        ("render_720_wireframe_ms", Some(2)),
    ] {
        let mut values: Vec<f64> = results
            .iter()
            .map(|value| index.map_or(value.decode_ms, |index| value.render_ms[index]))
            .collect();
        values.sort_by(f64::total_cmp);
        let p50 = values[(samples as f64 * 0.50).ceil() as usize - 1];
        let p95 = values[(samples as f64 * 0.95).ceil() as usize - 1];
        println!(
            "# summary metric={metric}; n={samples}; nearest_rank_p50_ms={p50:.6}; nearest_rank_p95_ms={p95:.6}; max_ms={:.6}",
            values[samples - 1]
        );
    }
    Ok(())
}
