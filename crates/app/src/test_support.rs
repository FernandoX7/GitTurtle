//! What the app's tests share beyond any one module: the test binary's
//! counting allocator, so any test can tell that a path allocates nothing
//! ([`allocations`]).

/// The test binary's allocator: the system's, counting each thread's
/// allocations ([`allocations`]). Each allocation costs one thread-local
/// increment.
struct CountingAllocator;

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

thread_local! {
    static ALLOCATIONS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// The allocations this thread has made so far: a path allocates nothing
/// where the count is the same after it as before.
pub(crate) fn allocations() -> u64 {
    ALLOCATIONS.with(std::cell::Cell::get)
}

fn count_allocation() {
    // A const `Cell` has no destructor, so this never allocates and never
    // fails; `try_with` keeps a late allocation during a thread's teardown
    // from panicking all the same.
    let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
}

// SAFETY: each method passes its call to the system allocator unchanged, so
// the system allocator's guarantees hold.
unsafe impl std::alloc::GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        count_allocation();
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract.
        unsafe { std::alloc::System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: std::alloc::Layout) -> *mut u8 {
        count_allocation();
        // SAFETY: the caller upholds `GlobalAlloc::alloc_zeroed`'s contract.
        unsafe { std::alloc::System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: std::alloc::Layout, size: usize) -> *mut u8 {
        count_allocation();
        // SAFETY: the caller upholds `GlobalAlloc::realloc`'s contract.
        unsafe { std::alloc::System.realloc(pointer, layout, size) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: std::alloc::Layout) {
        // SAFETY: the caller upholds `GlobalAlloc::dealloc`'s contract.
        unsafe { std::alloc::System.dealloc(pointer, layout) }
    }
}
