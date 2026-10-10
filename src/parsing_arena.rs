use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::Once;

const ALIGN: usize = 16;
const CHUNK_BYTES: usize = 2 * 1024 * 1024;
const BIN_COUNT: usize = 28;

struct Arena {
    chunks: Vec<(*mut u8, usize, usize)>,
    bins: [Vec<*mut u8>; BIN_COUNT],
}

impl Arena {
    fn new() -> Self {
        Self {
            chunks: Vec::new(),
            bins: [(); BIN_COUNT].map(|()| Vec::new()),
        }
    }

    fn alloc(&mut self, user: usize) -> *mut u8 {
        let Some(cap) = class_cap(user) else {
            return std::ptr::null_mut();
        };
        if let Some(idx) = bin_index(cap)
            && let Some(ptr) = self.bins[idx].pop()
        {
            return ptr;
        }
        let total = ALIGN + cap;
        if self.chunks.last().is_none_or(|(_, chunk_cap, offset)| {
            offset.saturating_add(total) > *chunk_cap
        }) {
            self.push_chunk(total.max(CHUNK_BYTES));
        }
        let Some((base, chunk_cap, offset)) = self.chunks.last_mut() else {
            return std::ptr::null_mut();
        };
        if offset.saturating_add(total) > *chunk_cap {
            return std::ptr::null_mut();
        }
        let raw = unsafe { base.add(*offset) };
        *offset += total;
        unsafe {
            raw.cast::<usize>().write(cap);
            raw.add(ALIGN)
        }
    }

    fn free(&mut self, user: *mut u8) {
        if user.is_null() {
            return;
        }
        let raw = unsafe { user.sub(ALIGN) };
        let cap = unsafe { raw.cast::<usize>().read() };
        if let Some(idx) = bin_index(cap) {
            self.bins[idx].push(user);
        }
    }

    fn push_chunk(&mut self, cap: usize) {
        let Ok(layout) = std::alloc::Layout::from_size_align(cap, ALIGN) else {
            return;
        };
        let ptr = unsafe { std::alloc::alloc(layout) };
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        self.chunks.push((ptr, cap, 0));
    }
}

fn class_cap(user: usize) -> Option<usize> {
    let request = user.max(1);
    if request > (usize::MAX / 4) {
        return None;
    }
    Some(request.next_power_of_two().max(ALIGN))
}

fn bin_index(cap: usize) -> Option<usize> {
    if cap == 0 {
        return None;
    }
    let mut n = cap;
    let mut idx = 0usize;
    while n > 1 {
        n >>= 1;
        idx += 1;
        if idx >= BIN_COUNT {
            return None;
        }
    }
    Some(idx)
}

thread_local! {
    static ARENA: RefCell<Arena> = RefCell::new(Arena::new());
}

unsafe extern "C" fn arena_malloc(size: usize) -> *mut c_void {
    ARENA.with(|arena| arena.borrow_mut().alloc(size).cast())
}

unsafe extern "C" fn arena_calloc(count: usize, size: usize) -> *mut c_void {
    let Some(bytes) = count.checked_mul(size) else {
        return std::ptr::null_mut();
    };
    let ptr = unsafe { arena_malloc(bytes) };
    if !ptr.is_null() && bytes > 0 {
        unsafe { std::ptr::write_bytes(ptr, 0, bytes) };
    }
    ptr
}

unsafe extern "C" fn arena_realloc(ptr: *mut c_void, size: usize) -> *mut c_void {
    if ptr.is_null() {
        return unsafe { arena_malloc(size) };
    }
    let old_cap = unsafe { ptr.cast::<u8>().sub(ALIGN).cast::<usize>().read() };
    if size <= old_cap {
        return ptr;
    }
    let new_ptr = unsafe { arena_malloc(size) };
    if new_ptr.is_null() {
        return std::ptr::null_mut();
    }
    unsafe {
        std::ptr::copy_nonoverlapping(ptr.cast::<u8>(), new_ptr.cast::<u8>(), old_cap.min(size));
        arena_free(ptr);
    }
    new_ptr
}

unsafe extern "C" fn arena_free(ptr: *mut c_void) {
    ARENA.with(|arena| arena.borrow_mut().free(ptr.cast()));
}

pub(crate) fn install() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        tree_sitter::set_allocator(
            Some(arena_malloc),
            Some(arena_calloc),
            Some(arena_realloc),
            Some(arena_free),
        );
    });
}
