//! In-process macOS `QLPreviewPanel` (`QuickLookUI.framework`) presentation for
//! captured revision bytes.
//!
//! Files are always written off the UI thread by `rich_preview::captured_copy`
//! into a private `0o700` temporary directory with a fixed `0o400` filename.
//! Presenting on the main thread keeps Quick Look within the active Space and
//! focus hierarchy without spawning external `/usr/bin/qlmanage` helper windows.

use block2::RcBlock;
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, define_class, msg_send,
    rc::Retained,
    runtime::{AnyClass, AnyObject, Bool, Imp, NSObject, NSObjectProtocol, ProtocolObject, Sel},
    sel,
};
use objc2_app_kit::{NSApplication, NSWindowWillCloseNotification};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSString, NSURL};
use std::{
    cell::RefCell,
    ffi::CStr,
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    ptr::NonNull,
};
use tempfile::TempDir;

#[link(name = "QuickLookUI", kind = "framework")]
unsafe extern "C" {
    #[link_name = "OBJC_CLASS_$_QLPreviewPanel"]
    static QL_PREVIEW_PANEL_CLASS: std::ffi::c_void;
}

#[inline]
fn ql_preview_panel_class() -> Option<&'static AnyClass> {
    // Anchor the framework symbol so `ld` retains `-framework QuickLookUI`,
    // then resolve the Objective-C class safely through the runtime.
    let _ = &raw const QL_PREVIEW_PANEL_CLASS;
    AnyClass::get(c"QLPreviewPanel")
}

struct ItemIvars {
    url: Retained<NSURL>,
    title: Retained<NSString>,
}

define_class!(
    // SAFETY: `NSObject` has no subclassing requirements and `GitTurtleQuickLookItem`
    // does not implement `Drop`; `define_class!` drops its ivars on `dealloc`.
    #[unsafe(super(NSObject))]
    #[ivars = ItemIvars]
    struct GitTurtleQuickLookItem;

    unsafe impl NSObjectProtocol for GitTurtleQuickLookItem {}

    impl GitTurtleQuickLookItem {
        #[unsafe(method_id(previewItemURL))]
        fn preview_item_url(&self) -> Retained<NSURL> {
            self.ivars().url.clone()
        }

        #[unsafe(method_id(previewItemTitle))]
        fn preview_item_title(&self) -> Retained<NSString> {
            self.ivars().title.clone()
        }
    }
);

impl GitTurtleQuickLookItem {
    fn new(path: &std::path::Path, title: &str) -> Option<Retained<Self>> {
        let path_str = path.to_str()?;
        let ns_path = NSString::from_str(path_str);
        let url = NSURL::fileURLWithPath_isDirectory(&ns_path, false);
        let title = NSString::from_str(title);
        let this = Self::alloc().set_ivars(ItemIvars { url, title });
        // SAFETY: `NSObject`'s `init` is its designated initializer.
        Some(unsafe { msg_send![super(this), init] })
    }
}

define_class!(
    // SAFETY: `NSObject` has no subclassing requirements and holds no ivars.
    #[unsafe(super(NSObject))]
    struct GitTurtleQuickLookDataSource;

    unsafe impl NSObjectProtocol for GitTurtleQuickLookDataSource {}

    impl GitTurtleQuickLookDataSource {
        #[unsafe(method(numberOfPreviewItemsInPreviewPanel:))]
        fn number_of_preview_items(&self, _panel: *mut AnyObject) -> isize {
            catch_unwind(AssertUnwindSafe(|| {
                PANEL_STATE.with(|state| {
                    state
                        .try_borrow()
                        .ok()
                        .and_then(|s| s.item.as_ref().map(|_| 1))
                        .unwrap_or(0)
                })
            }))
            .unwrap_or(0)
        }

        #[unsafe(method_id(previewPanel:previewItemAtIndex:))]
        fn preview_item_at_index(
            &self,
            _panel: *mut AnyObject,
            index: isize,
        ) -> Option<Retained<GitTurtleQuickLookItem>> {
            if index != 0 {
                return None;
            }
            catch_unwind(AssertUnwindSafe(|| {
                PANEL_STATE.with(|state| state.try_borrow().ok().and_then(|s| s.item.clone()))
            }))
            .unwrap_or(None)
        }
    }
);

impl GitTurtleQuickLookDataSource {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(());
        // SAFETY: `NSObject`'s `init` is its designated initializer.
        unsafe { msg_send![super(this), init] }
    }
}

struct CloseObserver {
    center: Retained<NSNotificationCenter>,
    token: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}

impl Drop for CloseObserver {
    fn drop(&mut self) {
        let token: &ProtocolObject<dyn NSObjectProtocol> = &self.token;
        // SAFETY: `token` was returned by `addObserverForName_object_queue_usingBlock`
        // on this retained `NSNotificationCenter`.
        unsafe { self.center.removeObserver(token.as_ref()) };
    }
}

struct PanelState {
    responder_installed: bool,
    presenting: bool,
    data_source: Option<Retained<GitTurtleQuickLookDataSource>>,
    item: Option<Retained<GitTurtleQuickLookItem>>,
    directory: Option<TempDir>,
    close_observer: Option<CloseObserver>,
}

thread_local! {
    static PANEL_STATE: RefCell<PanelState> = const {
        RefCell::new(PanelState {
            responder_installed: false,
            presenting: false,
            data_source: None,
            item: None,
            directory: None,
            close_observer: None,
        })
    };
}

struct PresentingGuard;

impl Drop for PresentingGuard {
    fn drop(&mut self) {
        PANEL_STATE.with(|state| {
            if let Ok(mut state) = state.try_borrow_mut() {
                state.presenting = false;
            }
        });
    }
}

fn drop_temp_dir_off_main(dir: Option<TempDir>) {
    if let Some(dir) = dir {
        let _ = std::thread::Builder::new()
            .name("gitturtle-ql-cleanup".into())
            .spawn(move || drop(dir));
    }
}

fn clear_active_item() {
    let old_dir = PANEL_STATE.with(|state| {
        let Ok(mut state) = state.try_borrow_mut() else {
            return None;
        };
        if state.presenting {
            return None;
        }
        state.item = None;
        state.directory.take()
    });
    drop_temp_dir_off_main(old_dir);
}

unsafe extern "C-unwind" fn accepts_preview_panel_control(
    _this: *mut AnyObject,
    _cmd: Sel,
    _panel: *mut AnyObject,
) -> Bool {
    Bool::YES
}

unsafe extern "C-unwind" fn begin_preview_panel_control(
    _this: *mut AnyObject,
    _cmd: Sel,
    panel: *mut AnyObject,
) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if panel.is_null() {
            return;
        }
        let Some(data_source) = PANEL_STATE.with(|state| {
            let mut state = state.try_borrow_mut().ok()?;
            Some(
                state
                    .data_source
                    .get_or_insert_with(GitTurtleQuickLookDataSource::new)
                    .clone(),
            )
        }) else {
            return;
        };
        // SAFETY: `panel` is the active `QLPreviewPanel` passed by AppKit on the
        // main thread; `data_source` is retained inside `PANEL_STATE` for the
        // controller's lifetime and no `RefCell` borrow is held across the call.
        unsafe {
            let ds_ptr: *const GitTurtleQuickLookDataSource = Retained::as_ptr(&data_source);
            let _: () = msg_send![panel, setDataSource: ds_ptr];
        }
    }));
}

unsafe extern "C-unwind" fn end_preview_panel_control(
    _this: *mut AnyObject,
    _cmd: Sel,
    panel: *mut AnyObject,
) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if !panel.is_null() {
            // SAFETY: `panel` is the active `QLPreviewPanel` releasing control on
            // the main thread.
            unsafe {
                let _: () = msg_send![panel, setDataSource: std::ptr::null::<AnyObject>()];
            }
        }
        clear_active_item();
    }));
}

fn install_responder_methods(mtm: MainThreadMarker) -> bool {
    if PANEL_STATE.with(|state| state.try_borrow().is_ok_and(|s| s.responder_installed)) {
        return true;
    }
    let app = NSApplication::sharedApplication(mtm);
    let Some(delegate) = app.delegate() else {
        return false;
    };
    let delegate_obj: &AnyObject = delegate.as_ref();
    let delegate_cls = delegate_obj.class();
    if delegate_cls.name() != c"GPUIApplicationDelegate" {
        return false;
    }

    // SAFETY: `GPUIApplicationDelegate` is an app-owned `NSResponder` subclass
    // at the end of AppKit's responder chain. Adding the three Quick Look
    // controller selectors once gives `QLPreviewPanel` a stable controller
    // without mutating vendored GPUI sources.
    let installed = unsafe {
        let cls = std::ptr::from_ref(delegate_cls).cast_mut();
        #[cfg(target_arch = "aarch64")]
        let bool_encoding: &CStr = c"B@:@";
        #[cfg(not(target_arch = "aarch64"))]
        let bool_encoding: &CStr = c"c@:@";
        let void_encoding: &CStr = c"v@:@";

        let accepts_imp: Imp = std::mem::transmute::<
            unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> Bool,
            Imp,
        >(accepts_preview_panel_control);
        let begin_imp: Imp = std::mem::transmute::<
            unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject),
            Imp,
        >(begin_preview_panel_control);
        let end_imp: Imp = std::mem::transmute::<
            unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject),
            Imp,
        >(end_preview_panel_control);

        let accepts_sel = sel!(acceptsPreviewPanelControl:);
        let begin_sel = sel!(beginPreviewPanelControl:);
        let end_sel = sel!(endPreviewPanelControl:);

        let _ = objc2::ffi::class_addMethod(cls, accepts_sel, accepts_imp, bool_encoding.as_ptr());
        let _ = objc2::ffi::class_addMethod(cls, begin_sel, begin_imp, void_encoding.as_ptr());
        let _ = objc2::ffi::class_addMethod(cls, end_sel, end_imp, void_encoding.as_ptr());

        delegate_cls.instance_method(accepts_sel).is_some()
            && delegate_cls.instance_method(begin_sel).is_some()
            && delegate_cls.instance_method(end_sel).is_some()
    };

    if installed {
        PANEL_STATE.with(|state| {
            if let Ok(mut state) = state.try_borrow_mut() {
                state.responder_installed = true;
            }
        });
    }
    installed
}

fn ensure_close_observer(panel: *mut AnyObject) {
    if panel.is_null()
        || PANEL_STATE.with(|state| state.try_borrow().is_ok_and(|s| s.close_observer.is_some()))
    {
        return;
    }
    let center = NSNotificationCenter::defaultCenter();
    let callback = RcBlock::new(|_: NonNull<NSNotification>| {
        let _ = catch_unwind(AssertUnwindSafe(clear_active_item));
    });
    // Deliver synchronously on the posting (main) thread (`queue: None`) so a
    // close notification cannot run asynchronously after a newer `present()`.
    // No `PANEL_STATE` borrow is held across this Objective-C call.
    //
    // SAFETY: `panel` is a valid `QLPreviewPanel` (`NSPanel` / `NSWindow`) on
    // the main thread; dropping `CloseObserver` removes the returned token.
    let token = unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(NSWindowWillCloseNotification),
            Some(&*panel),
            None,
            &callback,
        )
    };
    let replaced = PANEL_STATE.with(|state| {
        state
            .try_borrow_mut()
            .ok()
            .and_then(|mut s| s.close_observer.replace(CloseObserver { center, token }))
    });
    drop(replaced);
}

/// Presents a captured read-only file inside macOS's in-process `QLPreviewPanel`.
/// Must be called on the main thread. Returns `Ok(())` when presented in-process,
/// or `Err((directory, path))` if `QLPreviewPanel` is unexpectedly unavailable
/// so the caller can clean up or fall back.
pub(crate) fn present(
    directory: TempDir,
    path: PathBuf,
    title: &str,
) -> std::result::Result<(), (TempDir, PathBuf)> {
    let Some(mtm) = MainThreadMarker::new() else {
        return Err((directory, path));
    };
    if !install_responder_methods(mtm) {
        return Err((directory, path));
    }
    let Some(panel_class) = ql_preview_panel_class() else {
        return Err((directory, path));
    };

    // SAFETY: `panel_class` is `QLPreviewPanel` resolved through the Objective-C
    // runtime and `sharedPreviewPanel` is main-thread AppKit API.
    let panel: *mut AnyObject = unsafe { msg_send![panel_class, sharedPreviewPanel] };
    if panel.is_null() {
        return Err((directory, path));
    }

    let Some(item) = GitTurtleQuickLookItem::new(&path, title) else {
        return Err((directory, path));
    };
    ensure_close_observer(panel);

    let (data_source, previous_dir) = PANEL_STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.presenting = true;
        let ds = state
            .data_source
            .get_or_insert_with(GitTurtleQuickLookDataSource::new)
            .clone();
        state.item = Some(item);
        let old = state.directory.replace(directory);
        (ds, old)
    });
    let _presenting = PresentingGuard;

    // Order the panel front and synchronize its controller before reloading so
    // both first-open (`beginPreviewPanelControl:`) and already-open item
    // replacement display the staged item without racing against a close event.
    //
    // SAFETY: `panel` is non-null on the main thread; `data_source` stays alive
    // in `PANEL_STATE` until `endPreviewPanelControl:` or `close_on_quit`, and
    // no `PANEL_STATE` borrow is held across these Objective-C calls.
    unsafe {
        let ds_ptr: *const GitTurtleQuickLookDataSource = Retained::as_ptr(&data_source);
        let _: () = msg_send![panel, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
        let _: () = msg_send![panel, updateController];
        let _: () = msg_send![panel, setDataSource: ds_ptr];
        let _: () = msg_send![panel, reloadData];
        let _: () = msg_send![panel, refreshCurrentPreviewItem];
    }

    drop(_presenting);
    // Drop any replaced TempDir off the main thread AFTER `reloadData` has
    // switched `QuickLookUIService` to the new captured file.
    drop_temp_dir_off_main(previous_dir);
    Ok(())
}

/// Closes any open `QLPreviewPanel` and cleans up its staged temporary directory
/// during application shutdown.
pub(crate) fn close_on_quit() {
    if MainThreadMarker::new().is_some()
        && let Some(panel_class) = ql_preview_panel_class()
    {
        // SAFETY: `sharedPreviewPanelExists` avoids lazily instantiating a panel
        // on quit when Quick Look was never opened during the session. When it
        // exists on the main thread, `sharedPreviewPanel` returns the active
        // panel whose data source can be cleared and ordered out safely.
        unsafe {
            let exists: Bool = msg_send![panel_class, sharedPreviewPanelExists];
            if exists.as_bool() {
                let panel: *mut AnyObject = msg_send![panel_class, sharedPreviewPanel];
                if !panel.is_null() {
                    let _: () = msg_send![panel, orderOut: std::ptr::null::<AnyObject>()];
                    let _: () = msg_send![panel, setDataSource: std::ptr::null::<AnyObject>()];
                }
            }
        }
    }
    let (old_observer, old_dir) = PANEL_STATE.with(|state| {
        let Ok(mut state) = state.try_borrow_mut() else {
            return (None, None);
        };
        state.item = None;
        (state.close_observer.take(), state.directory.take())
    });
    // Drop outside the `RefCell` borrow (`CloseObserver::drop` calls ObjC).
    drop(old_observer);
    drop(old_dir);
}
