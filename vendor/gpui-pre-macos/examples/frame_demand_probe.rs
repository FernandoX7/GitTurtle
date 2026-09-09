//! No windows or display-link registration: exercise the existing dispatch source.
#[allow(dead_code)] // The probe exercises demand; the unused display registration stays included.
#[path = "../src/display_link.rs"]
mod display_link;
use display_link::WindowFrameSource;
use std::{
    ffi::c_void,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};
extern "C" fn count(data: *mut c_void) {
    // The source is cancelled before the stack-owned counter leaves main.
    unsafe { &*(data as *const AtomicUsize) }.fetch_add(1, Ordering::SeqCst);
}
fn pump(duration: Duration) {
    let until = Instant::now() + duration;
    while Instant::now() < until {
        unsafe {
            core_foundation_sys::runloop::CFRunLoopRunInMode(
                core_foundation_sys::runloop::kCFRunLoopDefaultMode,
                0.01,
                0,
            );
        }
        std::thread::yield_now();
    }
}
fn main() {
    let calls = AtomicUsize::new(0);
    let source = WindowFrameSource::new((&calls as *const AtomicUsize).cast_mut().cast(), count);
    for _ in 0..1000 {
        source.request_frame();
    }
    pump(Duration::from_millis(50));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "demand must coalesce without a display link"
    );
    pump(Duration::from_millis(50));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "idle source must not continue waking"
    );
    source.request_frame();
    drop(source);
    pump(Duration::from_millis(50));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "drop must cancel a queued callback"
    );
    println!("PASS: 1000 requests coalesced, no idle wakeups, queued callback cancelled on drop");
}
