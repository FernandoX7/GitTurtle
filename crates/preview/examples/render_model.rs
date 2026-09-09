//! Render supplied model bytes to four PNGs for decoder/presentation inspection.
use anyhow::{Context, Result, ensure};
use gitturtle_preview::{MAX_INPUT_BYTES, model3d};
use std::{io::Read, path::PathBuf};

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let source = PathBuf::from(
        args.next()
            .context("usage: render_model MODEL OUTPUT_DIRECTORY")?,
    );
    let output = PathBuf::from(
        args.next()
            .context("usage: render_model MODEL OUTPUT_DIRECTORY")?,
    );
    ensure!(
        args.next().is_none(),
        "usage: render_model MODEL OUTPUT_DIRECTORY"
    );
    let mut bytes = Vec::new();
    std::fs::File::open(&source)?
        .take((MAX_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    let preview = model3d::decode_model(&bytes, &source.to_string_lossy(), || Ok(()))?;
    std::fs::create_dir_all(&output)?;
    println!("{}: {}", preview.format, preview.details.join("\n"));
    for view in preview.views {
        let path = output.join(format!("{}.png", view.caption.to_ascii_lowercase()));
        image::save_buffer(
            &path,
            &view.image.rgba,
            view.image.width,
            view.image.height,
            image::ColorType::Rgba8,
        )?;
        println!("{}", path.display());
    }
    Ok(())
}
