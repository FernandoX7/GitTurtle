//! Bounded, composited GIF frames. Decoding never reads the hinted source path.
use super::*;
use image::{AnimationDecoder, codecs::gif::GifDecoder};

pub const MAX_FRAMES: usize = 120;
pub const MAX_DURATION_MS: u64 = 30_000;
pub const MAX_OUTPUT_PIXELS: u64 = 16_000_000;
pub const MAX_DECODED_PIXELS: u64 = 128_000_000;
pub const MAX_EDGE: u32 = 800;
pub const MAX_DECODE_SECONDS: u64 = 5;

pub struct AnimationFrame {
    pub image: ImagePreview,
    pub delay_ms: u32,
}

pub struct AnimationPreview {
    pub frames: Vec<AnimationFrame>,
    pub duration_ms: u64,
    /// A resource limit stopped traversal before the decoder reached the end.
    pub truncated: bool,
}

pub fn is_gif(bytes: &[u8]) -> bool {
    bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")
}

pub fn decode_gif(
    bytes: &[u8],
    max_edge: u32,
    check: impl Fn() -> Result<()>,
) -> Result<AnimationPreview> {
    ensure!(
        bytes.len() <= MAX_INPUT_BYTES,
        "GIF exceeds the 32 MiB input limit"
    );
    ensure!(is_gif(bytes), "Unrecognized GIF header");
    ensure!(max_edge > 0, "Preview size must be greater than zero");
    check()?;
    let started = std::time::Instant::now();
    let mut decoder = GifDecoder::new(Cursor::new(bytes)).context("Cannot read GIF header")?;
    let (original_width, original_height) = decoder.dimensions();
    check_dimensions(original_width, original_height)?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_EDGE);
    limits.max_image_height = Some(MAX_SOURCE_EDGE);
    limits.max_alloc = Some(MAX_DECODER_BYTES);
    decoder.set_limits(limits)?;
    let (width, height) =
        scaled_dimensions(original_width, original_height, max_edge.min(MAX_EDGE));
    let source_pixels = u64::from(original_width) * u64::from(original_height);
    let output_pixels = u64::from(width) * u64::from(height);
    let mut source = decoder.into_frames();
    let mut frames = Vec::new();
    let mut duration_ms = 0u64;
    let mut truncated = false;
    loop {
        check()?;
        let next_count = frames.len() as u64 + 1;
        if frames.len() == MAX_FRAMES
            || duration_ms >= MAX_DURATION_MS
            || started.elapsed().as_secs() >= MAX_DECODE_SECONDS
            || source_pixels.saturating_mul(next_count) > MAX_DECODED_PIXELS
            || output_pixels.saturating_mul(next_count) > MAX_OUTPUT_PIXELS
        {
            truncated = true;
            break;
        }
        let Some(frame) = source.next() else {
            break;
        };
        let frame = frame.context("Cannot decode GIF animation frame")?;
        let (numerator, denominator) = frame.delay().numer_denom_ms();
        // Zero/tiny GIF delays otherwise request a tight presentation loop.
        let delay_ms = u64::from(numerator)
            .div_ceil(u64::from(denominator).max(1))
            .max(20);
        let remaining = MAX_DURATION_MS - duration_ms;
        if !frames.is_empty() && delay_ms > remaining {
            truncated = true;
            break;
        }
        let delay_ms = delay_ms.min(remaining) as u32;
        let mut rgba = frame.into_buffer();
        ensure!(
            rgba.dimensions() == (original_width, original_height),
            "GIF compositor returned unexpected dimensions"
        );
        check()?;
        if rgba.dimensions() != (width, height) {
            premultiply(rgba.as_mut());
            rgba = image::imageops::resize(
                &rgba,
                width,
                height,
                image::imageops::FilterType::Triangle,
            );
            unpremultiply(rgba.as_mut());
        }
        frames.push(AnimationFrame {
            image: ImagePreview {
                width,
                height,
                original_width,
                original_height,
                rgba: rgba.into_raw(),
                format: "GIF".into(),
            },
            delay_ms,
        });
        duration_ms += u64::from(delay_ms);
    }
    ensure!(!frames.is_empty(), "GIF has no readable frames");
    Ok(AnimationPreview {
        frames,
        duration_ms,
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Delay, Frame, Rgba, RgbaImage, codecs::gif::GifEncoder};

    fn gif(count: usize, delay: u32, width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = GifEncoder::new(&mut bytes);
            for index in 0..count {
                encoder
                    .encode_frame(Frame::from_parts(
                        RgbaImage::from_pixel(
                            width,
                            height,
                            if index % 2 == 0 {
                                Rgba([255, 0, 0, 255])
                            } else {
                                Rgba([0, 255, 0, 255])
                            },
                        ),
                        0,
                        0,
                        Delay::from_numer_denom_ms(delay, 1),
                    ))
                    .unwrap();
            }
        }
        bytes
    }

    #[test]
    fn retains_distinct_composited_frames_delays_and_first_frame_api() {
        let bytes = gif(2, 70, 4, 2);
        let animation = decode_gif(&bytes, 2, || Ok(())).unwrap();
        assert_eq!(animation.frames.len(), 2);
        assert_eq!(animation.duration_ms, 140);
        assert!(!animation.truncated);
        assert_eq!(animation.frames[0].image.rgba[..4], [255, 0, 0, 255]);
        assert_eq!(animation.frames[1].image.rgba[..4], [0, 255, 0, 255]);
        assert_eq!(
            (
                animation.frames[0].image.width,
                animation.frames[0].image.height
            ),
            (2, 1)
        );
        let first = decode_image(&bytes, "first.gif", 2).unwrap();
        assert_eq!(first.rgba, animation.frames[0].image.rgba);
    }

    #[test]
    fn applies_transparent_overlay_and_background_disposal_before_next_frame() {
        let animation = decode_gif(
            include_bytes!("../tests/fixtures/disposal-transparency.gif"),
            4,
            || Ok(()),
        )
        .unwrap();
        assert_eq!(animation.frames.len(), 3);
        assert_eq!(animation.duration_ms, 270);
        let pixel = |frame: usize, x: usize| &animation.frames[frame].image.rgba[x * 4..x * 4 + 4];
        assert_eq!(pixel(0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel(0, 3), [255, 0, 0, 255]);
        assert_eq!(pixel(1, 0), [0, 255, 0, 255]);
        // Transparent pixels retain the previous frame until its disposal.
        assert_eq!(pixel(1, 3), [255, 0, 0, 255]);
        // Frame 1 restores the canvas background before frame 2 overlays blue.
        assert_eq!(pixel(2, 0)[3], 0);
        assert_eq!(pixel(2, 3), [0, 0, 255, 255]);
    }

    #[test]
    fn caps_frames_and_duration_and_cancels_before_more_decode() {
        let animation = decode_gif(&gif(MAX_FRAMES + 1, 20, 1, 1), 10, || Ok(())).unwrap();
        assert_eq!(animation.frames.len(), MAX_FRAMES);
        assert!(animation.truncated);
        let animation = decode_gif(&gif(5, 10_000, 1, 1), 10, || Ok(())).unwrap();
        assert_eq!(animation.frames.len(), 3);
        assert_eq!(animation.duration_ms, MAX_DURATION_MS);
        assert!(animation.truncated);
        let calls = std::cell::Cell::new(0);
        let result = decode_gif(&gif(5, 20, 1, 1), 10, || {
            calls.set(calls.get() + 1);
            ensure!(calls.get() < 5, "cancelled");
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(calls.get(), 5);
    }

    #[test]
    fn rejects_corrupt_oversized_headers_and_bounds_aggregate_output() {
        assert!(decode_gif(b"GIF89a", 100, || Ok(())).is_err());
        let mut bytes = gif(1, 20, 1, 1);
        bytes[6..10].copy_from_slice(&[255, 255, 255, 255]);
        assert!(decode_gif(&bytes, 100, || Ok(())).is_err());
        let animation = decode_gif(&gif(30, 20, 800, 800), 800, || Ok(())).unwrap();
        assert!(animation.truncated);
        assert_eq!(animation.frames.len(), 25);
        assert_eq!(
            animation
                .frames
                .iter()
                .map(|frame| frame.image.rgba.len() as u64 / 4)
                .sum::<u64>(),
            MAX_OUTPUT_PIXELS
        );
    }
}
