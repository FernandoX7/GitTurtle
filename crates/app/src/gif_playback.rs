//! Worker-prepared GIF pixels and an explicit, initially paused comparison clock.
use crate::gpui::{
    AnyElement, Bounds, Corners, ImageId, IntoElement, RenderImage, Styled, canvas, point, px, size,
};
use anyhow::{Result, ensure};
use gitturtle_preview::{ImagePreview, animation};
use std::{sync::Arc, time::Instant};

pub(super) struct Prepared {
    pub image: ImagePreview,
    pub render: Arc<RenderImage>,
    pub timeline: Arc<Timeline>,
}

/// Secondary inspectors are deliberately static. Paint the existing first
/// frame directly; GPUI's generic image element may schedule GIF animation.
pub(super) fn static_image(image: Arc<RenderImage>) -> AnyElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, cx| {
            crate::image_lifetime::track(&image, window, cx);
            let dimensions = image.size(0);
            let width = dimensions.width.0 as f32;
            let height = dimensions.height.0 as f32;
            if width <= 0. || height <= 0. {
                return;
            }
            let scale =
                (f32::from(bounds.size.width) / width).min(f32::from(bounds.size.height) / height);
            let image_size = size(px(width * scale), px(height * scale));
            let image_bounds = Bounds::new(
                bounds.origin
                    + point(
                        (bounds.size.width - image_size.width) / 2.,
                        (bounds.size.height - image_size.height) / 2.,
                    ),
                image_size,
            );
            let _ = window.paint_image(
                bounds,
                image_bounds,
                Corners::default(),
                image.clone(),
                0,
                false,
            );
        },
    )
    .size_full()
    .into_any_element()
}

#[derive(Debug)]
pub(super) struct Timeline {
    pub end_ms: Vec<u64>,
    pub duration_ms: u64,
    pub truncated: bool,
}

impl Timeline {
    pub fn retained_bytes(&self) -> usize {
        self.end_ms.capacity() * std::mem::size_of::<u64>()
    }
    /// Both sides use one comparison clock; the shorter side holds its last
    /// frame until the longer side completes and the comparison loops together.
    pub fn frame_at(&self, elapsed_ms: u64) -> usize {
        self.end_ms
            .partition_point(|end| *end <= elapsed_ms)
            .min(self.end_ms.len().saturating_sub(1))
    }
}

pub(super) fn prepare(bytes: &[u8], check: impl Fn() -> Result<()>) -> Result<Prepared> {
    let decoded = animation::decode_gif(bytes, animation::MAX_EDGE, &check)?;
    let mut image = decoded.frames[0].image.clone();
    image.format = format!(
        "GIF · {} frames{}",
        decoded.frames.len(),
        if decoded.truncated {
            " · bounded animation segment"
        } else {
            ""
        }
    );
    let mut end_ms = Vec::with_capacity(decoded.frames.len());
    let mut frames = Vec::with_capacity(decoded.frames.len());
    let mut elapsed = 0u64;
    for frame in decoded.frames {
        check()?;
        let mut bgra = frame.image.rgba;
        let (pixels, remainder) = bgra.as_chunks_mut::<4>();
        ensure!(remainder.is_empty(), "Invalid GIF pixel buffer");
        for pixel in pixels {
            pixel.swap(0, 2);
        }
        let buffer = image::RgbaImage::from_raw(frame.image.width, frame.image.height, bgra)
            .ok_or_else(|| anyhow::anyhow!("Invalid GIF preview dimensions"))?;
        frames.push(image::Frame::from_parts(
            buffer,
            0,
            0,
            image::Delay::from_numer_denom_ms(frame.delay_ms, 1),
        ));
        elapsed += u64::from(frame.delay_ms);
        end_ms.push(elapsed);
    }
    Ok(Prepared {
        image,
        render: Arc::new(RenderImage::new(frames)),
        timeline: Arc::new(Timeline {
            end_ms,
            duration_ms: decoded.duration_ms,
            truncated: decoded.truncated,
        }),
    })
}

pub(super) type SourceKey = [Option<ImageId>; 2];

#[derive(Default, Debug)]
pub(super) struct Playback {
    key: SourceKey,
    offset_ms: u64,
    started: Option<Instant>,
}

impl Clone for Playback {
    fn clone(&self) -> Self {
        // Navigation restoration must not revive a live animation callback.
        Self {
            key: self.key,
            offset_ms: self.elapsed(Instant::now()),
            started: None,
        }
    }
}

impl Playback {
    fn elapsed(&self, now: Instant) -> u64 {
        self.offset_ms
            .saturating_add(self.started.map_or(0, |start| {
                now.saturating_duration_since(start)
                    .as_millis()
                    .min(u128::from(u64::MAX)) as u64
            }))
    }
    pub fn position(&self, key: SourceKey, duration_ms: u64, now: Instant) -> u64 {
        if self.key != key || duration_ms == 0 {
            0
        } else {
            self.elapsed(now) % duration_ms
        }
    }
    pub fn playing(&self, key: SourceKey) -> bool {
        self.key == key && self.started.is_some()
    }
    pub fn toggle(&mut self, key: SourceKey, duration_ms: u64, now: Instant) {
        let position = self.position(key, duration_ms, now);
        let was_playing = self.playing(key);
        self.key = key;
        self.offset_ms = position;
        self.started = (!was_playing).then_some(now);
    }
    pub fn pause(&mut self, now: Instant) {
        self.offset_ms = self.elapsed(now);
        self.started = None;
    }
    pub fn seek(&mut self, key: SourceKey, position_ms: u64) {
        self.key = key;
        self.offset_ms = position_ms;
        self.started = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn prepares_all_bgra_frames_once_and_keeps_original_rgba_and_timing() {
        let mut bytes = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
            for pixel in [[255, 0, 0, 255], [0, 255, 0, 255]] {
                encoder
                    .encode_frame(image::Frame::from_parts(
                        image::RgbaImage::from_pixel(2, 1, image::Rgba(pixel)),
                        0,
                        0,
                        image::Delay::from_numer_denom_ms(80, 1),
                    ))
                    .unwrap();
            }
        }
        let prepared = prepare(&bytes, || Ok(())).unwrap();
        assert_eq!(prepared.render.frame_count(), 2);
        assert_eq!(prepared.render.as_bytes(0).unwrap()[..4], [0, 0, 255, 255]);
        assert_eq!(prepared.render.as_bytes(1).unwrap()[..4], [0, 255, 0, 255]);
        assert_eq!(prepared.image.rgba[..4], [255, 0, 0, 255]);
        assert_eq!(prepared.timeline.end_ms, [80, 160]);
    }
    #[test]
    fn explicit_clock_pauses_loops_and_never_applies_to_replaced_sources() {
        let key = [Some(ImageId(11)), Some(ImageId(12))];
        let now = Instant::now();
        let mut playback = Playback::default();
        assert!(!playback.playing(key));
        playback.toggle(key, 1000, now);
        assert_eq!(
            playback.position(key, 1000, now + Duration::from_millis(1250)),
            250
        );
        assert_eq!(playback.position([None, Some(ImageId(13))], 1000, now), 0);
        playback.toggle(key, 1000, now + Duration::from_millis(1250));
        assert!(!playback.playing(key));
        assert_eq!(
            playback.position(key, 1000, now + Duration::from_secs(20)),
            250
        );
        playback.seek(key, 700);
        assert_eq!(playback.position(key, 1000, now), 700);
        playback.toggle(key, 1000, now);
        assert!(!playback.clone().playing(key));
    }
    #[test]
    fn reduced_motion_pause_keeps_the_current_frame_until_explicit_resume() {
        let now = Instant::now();
        let key = [Some(ImageId(31)), None];
        let mut playback = Playback::default();
        playback.toggle(key, 1000, now);
        playback.pause(now + Duration::from_millis(240));
        assert!(!playback.playing(key));
        assert_eq!(
            playback.position(key, 1000, now + Duration::from_secs(30)),
            240
        );
    }
    #[test]
    fn unequal_durations_share_time_and_shorter_side_holds_last_frame() {
        let timeline = Timeline {
            end_ms: vec![70, 150, 500],
            duration_ms: 500,
            truncated: false,
        };
        assert_eq!(timeline.frame_at(69), 0);
        assert_eq!(timeline.frame_at(70), 1);
        assert_eq!(timeline.frame_at(151), 2);
        assert_eq!(timeline.frame_at(900), 2);
    }
}
