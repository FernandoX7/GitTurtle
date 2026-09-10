//! Supplied-byte release measurements for retained GLB appearance and animation.
//! No asset resource resolution. File reads/output destruction/GPU are excluded.
use anyhow::{Context, Result, ensure};
use gitturtle_preview::{MAX_INPUT_BYTES, model3d};
use std::{hint::black_box, io::Read, path::PathBuf, time::Instant};

fn main() -> Result<()> {
    ensure!(
        !cfg!(debug_assertions),
        "Build this example in release mode"
    );
    let mut args = std::env::args_os().skip(1);
    let path = PathBuf::from(
        args.next()
            .context("usage: bench_model_animation MODEL [SAMPLES=100] [WARMUPS=3] [CLIP=0]")?,
    );
    let samples = args
        .next()
        .map(|v| v.to_string_lossy().parse::<usize>())
        .transpose()?
        .unwrap_or(100);
    let warmups = args
        .next()
        .map(|v| v.to_string_lossy().parse::<usize>())
        .transpose()?
        .unwrap_or(3);
    let clip = args
        .next()
        .map(|v| v.to_string_lossy().parse::<usize>())
        .transpose()?
        .unwrap_or(0);
    ensure!(
        args.next().is_none() && (1..=10_000).contains(&samples) && warmups <= 1_000,
        "Invalid arguments"
    );
    let mut bytes = Vec::new();
    std::fs::File::open(&path)?
        .take((MAX_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= MAX_INPUT_BYTES, "Input exceeds 32 MiB");
    let name = path.to_string_lossy();
    println!(
        "# input_bytes={}; executable={:?}",
        bytes.len(),
        std::env::current_exe()?
    );
    println!(
        "# boundary=fresh supplied-byte decode and static 720 render; retained animation evaluation then 360/720 renders; excludes file I/O, camera fitting, output destruction, native/GPU/OS presentation"
    );
    println!(
        "# cache=input bytes resident; fresh scene each row; sample timestamps cycle through clip at 60 equal intervals; no app cache"
    );
    println!(
        "phase\tsample\tseconds\tdecode_ms\tstatic_720_ms\tfirst_usable_cpu_ms\tevaluate_ms\tframe_360_ms\tframe_720_ms\ttriangles\tscene_bytes\tpose_bytes"
    );
    for (phase, count) in [("first", 1), ("warmup", warmups), ("measured", samples)] {
        for i in 0..count {
            let start = Instant::now();
            let model = black_box(model3d::decode_geometry(black_box(&bytes), &name, || {
                Ok(())
            })?);
            let decode = start.elapsed().as_secs_f64() * 1e3;
            let camera = model3d::ModelCamera::fit(model.scene.bounds);
            let start = Instant::now();
            let image = black_box(model3d::render_model(
                &model.scene,
                &camera,
                720,
                false,
                || Ok(()),
            )?);
            let static_ms = start.elapsed().as_secs_f64() * 1e3;
            drop(image);
            let seconds = model
                .scene
                .animation_clips()
                .get(clip)
                .map_or(0., |clip| clip.duration_seconds * (i % 61) as f64 / 60.);
            let start = Instant::now();
            let evaluated = if model.scene.animation_clips().is_empty() {
                None
            } else {
                Some(black_box(model.scene.evaluate_animation(
                    clip,
                    seconds,
                    &|| Ok(()),
                )?))
            };
            let evaluate = start.elapsed().as_secs_f64() * 1e3;
            let pose = evaluated.as_ref().unwrap_or(&model.scene);
            let mut frame_ms = [0.; 2];
            for (index, edge) in [360, 720].into_iter().enumerate() {
                let start = Instant::now();
                let image = black_box(model3d::render_model(
                    black_box(pose),
                    &camera,
                    edge,
                    false,
                    || Ok(()),
                )?);
                frame_ms[index] = start.elapsed().as_secs_f64() * 1e3;
                ensure!(image.rgba.len() == edge as usize * edge as usize * 4);
                drop(image);
            }
            println!(
                "{phase}\t{i}\t{seconds:.6}\t{decode:.6}\t{static_ms:.6}\t{:.6}\t{evaluate:.6}\t{:.6}\t{:.6}\t{}\t{}\t{}",
                decode + static_ms,
                frame_ms[0],
                frame_ms[1],
                pose.triangle_count(),
                model.scene.retained_bytes(),
                pose.retained_bytes()
            );
        }
    }
    Ok(())
}
