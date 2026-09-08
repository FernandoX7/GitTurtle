//! Optional AppKit material confined to the navigation header. GPUI owns content.
//! Runtime class lookup keeps pre-macOS-26 systems on the opaque path.
#[cfg(target_os = "macos")]
mod macos {
    use gpui_kit::{Window, WindowBackgroundAppearance};
    use objc2::{MainThreadMarker, msg_send, rc::Retained, runtime::AnyClass};
    use objc2_app_kit::{NSView, NSWorkspace, NSWindowOrderingMode};
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    #[derive(Default)]
    pub(super) struct Material { view: Option<Retained<NSView>>, active: bool }
    impl Material {
        pub(super) fn update(&mut self, window: &Window, enabled: bool) -> bool {
            let workspace = NSWorkspace::sharedWorkspace();
            let allowed = enabled && window.is_window_active()
                && !workspace.accessibilityDisplayShouldReduceTransparency()
                && !workspace.accessibilityDisplayShouldIncreaseContrast();
            if !allowed {
                if let Some(view) = &self.view { view.setHidden(true); }
                if self.active { window.set_background_appearance(WindowBackgroundAppearance::Opaque); }
                self.active = false;
                return false;
            }
            let Some(class) = AnyClass::get(c"NSGlassEffectView") else { return false; };
            let Ok(handle) = HasWindowHandle::window_handle(window) else { return false; };
            let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return false; };
            let Some(_) = MainThreadMarker::new() else { return false; };
            // The raw handle belongs to this live GPUI window; all AppKit access stays on its UI thread.
            let native = unsafe { &*(handle.ns_view.as_ptr() as *const NSView) };
            let bounds = native.bounds();
            let frame = NSRect::new(NSPoint::new(0., if native.isFlipped() { 0. } else { (bounds.size.height - 56.).max(0.) }), NSSize::new(bounds.size.width, 56.));
            if self.view.is_none() {
                let view: Retained<NSView> = unsafe {
                    let allocated: *mut NSView = msg_send![class, alloc];
                    let initialized: *mut NSView = msg_send![allocated, initWithFrame: frame];
                    let view = Retained::from_raw(initialized).expect("AppKit glass allocation");
                    let _: () = msg_send![&*view, setCornerRadius: 0.0f64];
                    native.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Below, None);
                    view
                };
                self.view = Some(view);
            }
            if let Some(view) = &self.view { if view.frame() != frame { view.setFrame(frame); } view.setHidden(false); }
            if !self.active { window.set_background_appearance(WindowBackgroundAppearance::Transparent); }
            self.active = true;
            true
        }
    }
    impl Drop for Material { fn drop(&mut self) { if let Some(view) = &self.view { view.removeFromSuperview(); } } }
}

#[derive(Default)]
pub(super) struct Material {
    #[cfg(target_os = "macos")]
    inner: macos::Material,
    pub active: bool,
}
impl Material {
    pub fn update(&mut self, window: &gpui_kit::Window, enabled: bool) {
        #[cfg(target_os = "macos")]
        { self.active = self.inner.update(window, enabled); }
        #[cfg(not(target_os = "macos"))]
        { let _ = (window, enabled); self.active = false; }
    }
}
