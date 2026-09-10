//! Retire uploaded atlas tiles after every ordinary image owner has released them.
use crate::gpui::{App, Global, ImageId, RenderImage, Window, WindowId};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Default)]
struct Registry {
    // One cleanup reference, never one reference per paint/frame/window.
    images: HashMap<ImageId, Arc<RenderImage>>,
    pending: HashSet<WindowId>,
    trace: bool,
}
impl Global for Registry {}

impl Registry {
    fn track(&mut self, image: &Arc<RenderImage>) {
        self.images
            .entry(image.id)
            .or_insert_with(|| Arc::clone(image));
    }

    fn schedule(&mut self, window: WindowId) -> bool {
        !self.images.is_empty() && self.pending.insert(window)
    }

    fn released(&mut self) -> Vec<Arc<RenderImage>> {
        let ids: Vec<_> = self
            .images
            .iter()
            .filter(|(_, image)| Arc::strong_count(image) == 1)
            .map(|(id, _)| *id)
            .collect();
        ids.into_iter()
            .filter_map(|id| self.images.remove(&id))
            .collect()
    }
}

pub(super) fn init(cx: &mut App) {
    cx.set_global(Registry {
        trace: std::env::var_os("GITTURTLE_TRACE").is_some(),
        ..Default::default()
    });
    cx.on_window_closed(|cx, id| {
        // A callback queued on a closed window will never run. Other windows
        // must remain able to schedule cleanup, including for shared images.
        cx.global_mut::<Registry>().pending.remove(&id);
        // GPUI invokes this observer before dropping the Window box. App::defer
        // runs after that drop and release_dropped_entities, so its frame/view
        // references have gone before checking whether we are the sole owner.
        cx.defer(|cx| {
            retire(None, cx);
            let registry = cx.global::<Registry>();
            if registry.trace {
                eprintln!(
                    "gitturtle.image_window_closed retained_images={}",
                    registry.images.len()
                );
            }
        });
    })
    .detach();
}

/// Call during a real workspace render, including Back/Projects with no image.
/// A callback queued by the preceding frame may run before this render. This
/// hook then queues the one follow-up needed after its old frame is cleared.
pub(super) fn after_draw(window: &Window, cx: &mut App) {
    let id = window.window_handle().window_id();
    if !cx.global_mut::<Registry>().schedule(id) {
        return;
    }
    // In pinned GPUI the callback is delivered before the next platform draw.
    // Registered *during* this draw, it therefore follows this draw's frame
    // swap/clear and element-arena clear. It never invalidates a view or queues
    // itself: cleanup cannot create an idle render/poll loop.
    window.on_next_frame(move |window, cx| {
        cx.global_mut::<Registry>().pending.remove(&id);
        retire(Some(window), cx);
    });
}

/// Register the whole RenderImage before painting any of its frames.
pub(super) fn track(image: &Arc<RenderImage>, window: &Window, cx: &mut App) {
    if !cx.global::<Registry>().images.contains_key(&image.id) {
        cx.global_mut::<Registry>().track(image);
        let registry = cx.global::<Registry>();
        if registry.trace {
            eprintln!(
                "gitturtle.image_track frames={} retained_images={}",
                image.frame_count(),
                registry.images.len()
            );
        }
    }
    after_draw(window, cx);
}

fn retire(window: Option<&mut Window>, cx: &mut App) {
    let released = cx.global_mut::<Registry>().released();
    let count = released.len();
    let frames = released
        .iter()
        .map(|image| image.frame_count())
        .sum::<usize>();
    let mut window = window;
    for image in released {
        // This supported API removes every GIF frame in every window, including
        // the window currently removed from App.windows during its update.
        // A cache, retained inspection, dialog, canvas or another window holding
        // any Arc prevents retirement, so live cached scenes keep valid tiles.
        cx.drop_image(image, window.as_deref_mut());
    }
    let registry = cx.global::<Registry>();
    if count > 0 && registry.trace {
        eprintln!(
            "gitturtle.image_retire images={count} frames={frames} retained_images={}",
            registry.images.len()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::Registry;
    use crate::gpui::{RenderImage, WindowId};
    use std::sync::Arc;

    fn image(frames: usize) -> Arc<RenderImage> {
        Arc::new(RenderImage::new(
            (0..frames)
                .map(|_| image::Frame::new(image::RgbaImage::new(2, 2)))
                .collect::<Vec<_>>(),
        ))
    }

    #[test]
    fn repeated_paints_keep_one_cleanup_reference_and_one_callback_per_window() {
        let mut registry = Registry::default();
        let image = image(12);
        let window = WindowId::from(1);
        for _ in 0..100 {
            registry.track(&image);
        }
        assert_eq!(registry.images.len(), 1);
        assert_eq!(Arc::strong_count(&image), 2);
        assert!(registry.schedule(window));
        assert!(!registry.schedule(window));
        registry.pending.remove(&window);
        assert!(registry.released().is_empty());
        assert!(registry.pending.is_empty(), "a sweep must not rearm itself");
    }

    #[test]
    fn shared_cache_dialog_and_other_window_owners_protect_all_gif_frames() {
        let mut registry = Registry::default();
        let image = image(12);
        let cached = Arc::clone(&image);
        let dialog = Arc::clone(&image);
        let other_window = Arc::clone(&image);
        registry.track(&image);
        drop(image);
        drop(cached);
        assert!(registry.released().is_empty());
        drop(dialog);
        assert!(registry.released().is_empty());
        drop(other_window);
        let released = registry.released();
        assert_eq!(released.len(), 1);
        assert_eq!(
            released[0].frame_count(),
            12,
            "retire the full image, not just the displayed frame"
        );
        assert!(registry.images.is_empty());
        assert!(registry.released().is_empty());
    }

    #[test]
    fn back_render_rearms_once_after_a_delayed_old_frame_owner_is_cleared() {
        let mut registry = Registry::default();
        let source = image(1);
        let old_frame = Arc::clone(&source);
        let weak = Arc::downgrade(&source);
        let window = WindowId::from(1);
        registry.track(&source);
        assert!(registry.schedule(window));
        drop(source);
        // An earlier frame callback is delivered before Back's actual draw.
        registry.pending.remove(&window);
        assert!(registry.released().is_empty());
        // Back renders even though it has no new image. Its frame swap releases
        // old listeners/elements, then the newly scheduled callback can retire.
        assert!(registry.schedule(window));
        drop(old_frame);
        registry.pending.remove(&window);
        drop(registry.released());
        assert!(weak.upgrade().is_none());
        assert!(!registry.schedule(window));
        assert!(registry.pending.is_empty());
    }

    #[test]
    fn closing_one_window_does_not_strand_cleanup_or_retire_another_windows_image() {
        let mut registry = Registry::default();
        let first = WindowId::from(1);
        let second = WindowId::from(2);
        let first_image = image(1);
        let second_image = image(4);
        registry.track(&first_image);
        registry.track(&second_image);
        assert!(registry.schedule(first));
        registry.pending.remove(&first);
        drop(first_image);
        let retired = registry.released();
        assert_eq!(retired.len(), 1);
        assert_eq!(retired[0].frame_count(), 1);
        assert!(registry.schedule(second));
        drop(second_image);
        registry.pending.remove(&second);
        assert_eq!(registry.released()[0].frame_count(), 4);
        assert!(registry.images.is_empty());
    }

    #[test]
    fn sequential_worktree_opens_release_pixels_instead_of_accumulating_registry_owners() {
        let mut registry = Registry::default();
        for _ in 0..100 {
            let source = image(4);
            let weak = Arc::downgrade(&source);
            registry.track(&source);
            drop(source);
            drop(registry.released());
            assert!(weak.upgrade().is_none());
            assert!(registry.images.is_empty());
        }
    }
}
