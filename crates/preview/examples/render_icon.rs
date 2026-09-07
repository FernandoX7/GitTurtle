//! Rebuild the original app icon through GitTurtle's bounded SVG renderer:
//!
//! cargo run --locked -p gitturtle-preview --example render_icon -- \
//!   assets/app-icon.svg /tmp/GitTurtle.iconset
//! iconutil --convert icns --output assets/AppIcon.icns /tmp/GitTurtle.iconset

use anyhow::{Context, Result, ensure};
use std::{env, fs, path::PathBuf};

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let source = PathBuf::from(args.next().context("Expected source SVG path")?);
    let output = PathBuf::from(args.next().context("Expected output .iconset directory")?);
    ensure!(args.next().is_none(), "Expected exactly two arguments");
    ensure!(
        output
            .extension()
            .is_some_and(|extension| extension == "iconset"),
        "Output directory must end in .iconset"
    );
    let bytes = fs::read(source).context("Read app icon SVG")?;
    fs::create_dir_all(&output)?;
    for (points, scale) in [
        (16, 1),
        (16, 2),
        (32, 1),
        (32, 2),
        (128, 1),
        (128, 2),
        (256, 1),
        (256, 2),
        (512, 1),
        (512, 2),
    ] {
        let pixels = points * scale;
        let preview = gitturtle_preview::decode_image(&bytes, "app-icon.svg", pixels)?;
        ensure!(
            preview.width == pixels && preview.height == pixels,
            "App icon must be a square SVG of at least 1024 pixels"
        );
        let image = image::RgbaImage::from_raw(preview.width, preview.height, preview.rgba)
            .context("Invalid icon pixel buffer")?;
        let suffix = if scale == 2 { "@2x" } else { "" };
        let destination = output.join(format!("icon_{points}x{points}{suffix}.png"));
        image.save(&destination).context("Write icon PNG")?;
        println!("{} × {}  {}", pixels, pixels, destination.display());
    }
    Ok(())
}
