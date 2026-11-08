use std::alloc::{GlobalAlloc, Layout};
use std::ffi::c_void;

struct LibcAllocator;

unsafe extern "C" {
    fn posix_memalign(ptr: *mut *mut c_void, align: usize, size: usize) -> i32;
    fn free(ptr: *mut c_void);
}

unsafe impl GlobalAlloc for LibcAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut ptr = std::ptr::null_mut();
        let align = layout.align().max(size_of::<usize>());
        if unsafe { posix_memalign(&mut ptr, align, layout.size()) } != 0 {
            return std::ptr::null_mut();
        }
        ptr.cast()
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _: Layout) {
        unsafe { free(ptr.cast()) };
    }
}

#[global_allocator]
static ALLOCATOR: LibcAllocator = LibcAllocator;

const ERRNO_SUCCESS: i32 = 0;
const ERRNO_BADF: i32 = 8;
const ERRNO_SPIPE: i32 = 70;

#[repr(C)]
struct Ciovec {
    buf: *const u8,
    len: usize,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __imported_wasi_snapshot_preview1_environ_get(
    _: *mut *mut u8,
    _: *mut u8,
) -> i32 {
    ERRNO_SUCCESS
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __imported_wasi_snapshot_preview1_environ_sizes_get(
    count: *mut usize,
    size: *mut usize,
) -> i32 {
    unsafe {
        *count = 0;
        *size = 0;
    }
    ERRNO_SUCCESS
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __imported_wasi_snapshot_preview1_clock_time_get(
    id: u32,
    _precision: u64,
    time: *mut u64,
) -> i32 {
    let ns = if id == 0 {
        rbx_base::time::unix_seconds() * 1e9
    } else {
        static START: std::sync::OnceLock<rbx_base::time::Instant> = std::sync::OnceLock::new();
        START
            .get_or_init(rbx_base::time::Instant::now)
            .elapsed()
            .as_secs_f64()
            * 1e9
    };
    unsafe { *time = ns as u64 };
    ERRNO_SUCCESS
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __imported_wasi_snapshot_preview1_fd_close(_: i32) -> i32 {
    ERRNO_SUCCESS
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __imported_wasi_snapshot_preview1_fd_fdstat_get(
    fd: i32,
    stat: *mut u8,
) -> i32 {
    if !(0..=2).contains(&fd) {
        return ERRNO_BADF;
    }
    unsafe {
        std::ptr::write_bytes(stat, 0, 24);
        *stat = 2;
    }
    ERRNO_SUCCESS
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __imported_wasi_snapshot_preview1_fd_seek(
    _: i32,
    _: i64,
    _: i32,
    _: *mut u64,
) -> i32 {
    ERRNO_SPIPE
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __imported_wasi_snapshot_preview1_fd_write(
    fd: i32,
    iovs: *const Ciovec,
    count: usize,
    written: *mut usize,
) -> i32 {
    if !(1..=2).contains(&fd) {
        return ERRNO_BADF;
    }
    let mut bytes = Vec::new();
    for i in 0..count {
        let v = unsafe { &*iovs.add(i) };
        if v.len > 0 {
            bytes.extend_from_slice(unsafe { std::slice::from_raw_parts(v.buf, v.len) });
        }
    }
    let text = String::from_utf8_lossy(&bytes);
    let text = text.trim_end();
    if !text.is_empty() {
        if fd == 2 {
            tracing::warn!("{text}");
        } else {
            tracing::info!("{text}");
        }
    }
    unsafe { *written = bytes.len() };
    ERRNO_SUCCESS
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __imported_wasi_snapshot_preview1_proc_exit(code: i32) -> ! {
    panic!("C runtime exited with code {code}");
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __imported_wasi_snapshot_preview1_sched_yield() -> i32 {
    ERRNO_SUCCESS
}

unsafe extern "C" {
    fn __wasm_call_ctors();
}

pub fn init_c_runtime() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| unsafe { __wasm_call_ctors() });
}
