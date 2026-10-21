use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Counts {
    pub calls: u64,
    pub bytes: u64,
}

thread_local! {
    static ACTIVE: Cell<Option<Counts>> = const { Cell::new(None) };
}

#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

struct Allocator;

// The system allocator owns every pointer; instrumentation only records successful requests.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            record(layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            record(layout.size());
        }
        pointer
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let pointer = unsafe { System.realloc(pointer, layout, size) };
        if !pointer.is_null() {
            record(size);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
    }
}

fn record(size: usize) {
    let _ = ACTIVE.try_with(|active| {
        if let Some(mut counts) = active.get() {
            counts.calls = counts.calls.saturating_add(1);
            counts.bytes = counts.bytes.saturating_add(size as u64);
            active.set(Some(counts));
        }
    });
}

pub fn measure<T>(run: impl FnOnce() -> T) -> (T, Counts) {
    ACTIVE.with(|active| {
        assert!(active.get().is_none());
        active.set(Some(Counts::default()));
    });
    let guard = Guard;
    let result = run();
    let counts = ACTIVE.with(Cell::get).expect("Active allocation scope");
    drop(guard);
    (result, counts)
}

struct Guard;

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = ACTIVE.try_with(|active| active.set(None));
    }
}

#[test]
fn scopes_count_requests_and_stop_after_unwinding() {
    let (bytes, counts) = measure(|| std::hint::black_box(vec![0_u8; 4096]));
    assert_eq!(counts.calls, 1);
    assert_eq!(counts.bytes, 4096);
    assert_eq!(bytes.len(), 4096);
    assert!(std::panic::catch_unwind(|| measure(|| panic!("Scope cleanup"))).is_err());
    let (_, counts) = measure(|| std::hint::black_box(1_u32));
    assert_eq!(counts, Counts::default());
}
