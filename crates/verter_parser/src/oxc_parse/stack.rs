//! The stack a parse, or a walk of oxc's, runs on.
//!
//! [`with_stack`] runs work in place when the current stack has the bytes
//! the work can need, and otherwise on a stack segment reserved for it:
//! on Windows a fiber whose stack reserves the size and commits
//! [`INITIAL_COMMIT_BYTES`] of it (the system commits the rest page by
//! page as the work reaches it), on Unix an anonymous mapping the system
//! backs page by page, and on wasm32 a heap allocation holding the
//! module's shadow stack. A segment is sized from the work's nesting, so
//! a deep source reserves address space in proportion to its depth but
//! commits memory in proportion to the stack its work touches. A segment
//! that cannot be had is a [`StackUnavailable`], never a panic.

use std::cell::Cell;
use std::fmt;

/// A stack a parse or a walk needed and could not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StackUnavailable {
    /// The bytes of stack the work needed.
    pub needed: usize,
}

impl fmt::Display for StackUnavailable {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "the source nests deeper than a stack this host can provide ({} bytes needed)",
            self.needed
        )
    }
}

impl std::error::Error for StackUnavailable {}

/// The stack a Windows segment commits when it starts; the system commits
/// more, a page at a time, as the work on it grows the stack.
#[cfg(windows)]
pub const INITIAL_COMMIT_BYTES: usize = 64 * 1024;

thread_local! {
    /// The lowest usable address of the segment this thread runs on, while
    /// it runs on one of [`grow`]'s; `None` on the thread's own stack.
    static SEGMENT_LIMIT: Cell<Option<usize>> = const { Cell::new(None) };
}

fn stack_pointer() -> usize {
    psm::stack_pointer() as usize
}

/// The bytes left on the stack this thread runs on, when they are known.
pub fn remaining() -> Option<usize> {
    match SEGMENT_LIMIT.with(Cell::get) {
        Some(limit) => Some(stack_pointer().saturating_sub(limit)),
        None => thread_remaining(),
    }
}

/// Run `work` with at least `needed` bytes of stack: in place when the
/// stack this thread runs on has them, on a segment of `needed` bytes
/// otherwise.
pub fn with_stack<R>(needed: usize, work: impl FnOnce() -> R) -> Result<R, StackUnavailable> {
    if remaining().is_some_and(|left| left >= needed) {
        return Ok(work());
    }
    grow(needed, work)
}

/// Run `work` on the current segment's stack under `limit`, restoring the
/// enclosing stack's afterwards; a panic in `work` is carried out to the
/// caller of [`grow`], never unwound across the stack switch.
fn on_segment<R>(limit: usize, work: impl FnOnce() -> R) -> std::thread::Result<R> {
    let outer = SEGMENT_LIMIT.with(|segment| segment.replace(Some(limit)));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work));
    SEGMENT_LIMIT.with(|segment| segment.set(outer));
    result
}

fn resume<R>(result: std::thread::Result<R>) -> R {
    result.unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

#[cfg(not(target_arch = "wasm32"))]
fn thread_remaining() -> Option<usize> {
    stacker::remaining_stack()
}

/// The module's own stack, which the linker places between `__stack_low`
/// and `__stack_high`.
#[cfg(target_arch = "wasm32")]
fn thread_remaining() -> Option<usize> {
    unsafe extern "C" {
        static __stack_low: u8;
    }
    Some(stack_pointer().saturating_sub(&raw const __stack_low as usize))
}

#[cfg(windows)]
fn grow<R, F: FnOnce() -> R>(needed: usize, work: F) -> Result<R, StackUnavailable> {
    use std::ffi::c_void;
    use windows_sys::Win32::System::Threading::{
        ConvertFiberToThread, ConvertThreadToFiber, CreateFiberEx, DeleteFiber, IsThreadAFiber,
        SwitchToFiber,
    };

    struct Fiber<F, R> {
        work: Option<F>,
        result: Option<std::thread::Result<R>>,
        parent: *mut c_void,
    }

    unsafe extern "system" fn start<F: FnOnce() -> R, R>(data: *mut c_void) {
        // SAFETY: `data` is the `Fiber` `grow` keeps alive until this
        // fiber switches back to its parent, and only this fiber touches it
        // meanwhile.
        let fiber = unsafe { &mut *data.cast::<Fiber<F, R>>() };
        let work = fiber.work.take().expect("a fiber runs its work once");
        fiber.result = Some(on_segment(fiber_stack_limit(), work));
        // SAFETY: `parent` is the fiber that switched to this one; it is
        // alive, waiting in `grow`.
        unsafe { SwitchToFiber(fiber.parent) };
    }

    let unavailable = StackUnavailable { needed };
    // SAFETY: plain Win32 fiber calls; the fiber `CreateFiberEx` makes runs
    // `start`, which switches back before `grow` deletes it, and `fiber`
    // outlives it.
    unsafe {
        let converted = IsThreadAFiber() == 0;
        let parent = if converted {
            ConvertThreadToFiber(std::ptr::null())
        } else {
            current_fiber()
        };
        if parent.is_null() {
            return Err(unavailable);
        }
        let mut fiber = Fiber {
            work: Some(work),
            result: None,
            parent,
        };
        // The system's guard pages and stack-overflow guarantee come out of
        // the reservation.
        let reserve = needed.checked_add(fiber_overhead()).ok_or(unavailable)?;
        let handle = CreateFiberEx(
            INITIAL_COMMIT_BYTES,
            reserve,
            0,
            Some(start::<F, R>),
            (&raw mut fiber).cast(),
        );
        if !handle.is_null() {
            SwitchToFiber(handle);
            DeleteFiber(handle);
        }
        if converted {
            ConvertFiberToThread();
        }
        if handle.is_null() {
            return Err(unavailable);
        }
        Ok(resume(fiber.result.expect("the fiber ran its work")))
    }
}

/// The fiber this thread runs as: `GetCurrentFiber`, which Windows
/// defines inline, reading the thread information block's `FiberData`.
#[cfg(windows)]
unsafe fn current_fiber() -> *mut std::ffi::c_void {
    let fiber: *mut std::ffi::c_void;
    // SAFETY: the thread information block is mapped for every thread, at
    // the segment register (x86) or register (aarch64) Windows keeps it in.
    #[cfg(target_arch = "x86_64")]
    unsafe {
        std::arch::asm!("mov {}, gs:[0x20]", out(reg) fiber, options(nostack, readonly, preserves_flags));
    }
    #[cfg(target_arch = "x86")]
    unsafe {
        std::arch::asm!("mov {}, fs:[0x10]", out(reg) fiber, options(nostack, readonly, preserves_flags));
    }
    #[cfg(target_arch = "aarch64")]
    unsafe {
        std::arch::asm!("ldr {}, [x18, #0x20]", out(reg) fiber, options(nostack, readonly, preserves_flags));
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "x86", target_arch = "aarch64")))]
    {
        fiber = std::ptr::null_mut();
    }
    fiber
}

/// The bytes a Windows stack keeps from its reservation: the guarantee the
/// system holds back to handle a stack overflow, and a guard page.
#[cfg(windows)]
fn fiber_overhead() -> usize {
    use windows_sys::Win32::System::Threading::SetThreadStackGuarantee;
    let minimum = if cfg!(target_pointer_width = "32") {
        0x1000
    } else {
        0x2000
    };
    let mut guarantee = 0u32;
    // SAFETY: a zero guarantee asks for the current one without changing it.
    let read = unsafe { SetThreadStackGuarantee(&mut guarantee) } != 0;
    let guarantee = if read { guarantee as usize } else { 0 };
    guarantee.max(minimum) + 2 * 0x1000
}

/// The lowest address the current fiber's stack can use: the base of its
/// reservation above what [`fiber_overhead`] holds back.
#[cfg(windows)]
fn fiber_stack_limit() -> usize {
    use windows_sys::Win32::System::Memory::{VirtualQuery, MEMORY_BASIC_INFORMATION};
    let mut info = std::mem::MaybeUninit::<MEMORY_BASIC_INFORMATION>::uninit();
    // SAFETY: `info` is writable and as large as the call is told.
    let read = unsafe {
        VirtualQuery(
            stack_pointer() as *const _,
            info.as_mut_ptr(),
            std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    };
    if read == 0 {
        // The stack's base is unknown: nothing on it counts as spare.
        return stack_pointer();
    }
    // SAFETY: `VirtualQuery` filled `info`.
    let base = unsafe { info.assume_init() }.AllocationBase as usize;
    base + fiber_overhead()
}

#[cfg(all(unix, not(target_arch = "wasm32")))]
fn grow<R, F: FnOnce() -> R>(needed: usize, work: F) -> Result<R, StackUnavailable> {
    let unavailable = StackUnavailable { needed };
    // SAFETY: `sysconf` has no preconditions.
    let page = usize::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) })
        .ok()
        .filter(|page| page.is_power_of_two())
        .unwrap_or(4096);
    let usable = needed
        .checked_add(page - 1)
        .map(|size| size & !(page - 1))
        .ok_or(unavailable)?;
    // A guard page below the stack and one above it.
    let mapped = usable.checked_add(2 * page).ok_or(unavailable)?;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let flags = libc::MAP_PRIVATE | libc::MAP_ANON | libc::MAP_NORESERVE;
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    let flags = libc::MAP_PRIVATE | libc::MAP_ANON;
    // SAFETY: an anonymous private mapping of `mapped` bytes, unmapped
    // below on every path; the work runs on its middle pages only.
    unsafe {
        let mapping = libc::mmap(std::ptr::null_mut(), mapped, libc::PROT_NONE, flags, -1, 0);
        if mapping == libc::MAP_FAILED {
            return Err(unavailable);
        }
        let base = mapping.cast::<u8>().add(page);
        if libc::mprotect(base.cast(), usable, libc::PROT_READ | libc::PROT_WRITE) != 0 {
            libc::munmap(mapping, mapped);
            return Err(unavailable);
        }
        let result = psm::on_stack(base, usable, || on_segment(base as usize, work));
        libc::munmap(mapping, mapped);
        Ok(resume(result))
    }
}

#[cfg(target_arch = "wasm32")]
fn grow<R, F: FnOnce() -> R>(needed: usize, work: F) -> Result<R, StackUnavailable> {
    let unavailable = StackUnavailable { needed };
    let size = needed.checked_add(15).ok_or(unavailable)? & !15;
    let layout = std::alloc::Layout::from_size_align(size, 16).map_err(|_| unavailable)?;
    // SAFETY: `layout` has a non-zero size; the allocation is freed below
    // after the work, the only user of it, returns.
    unsafe {
        let base = std::alloc::alloc(layout);
        if base.is_null() {
            return Err(unavailable);
        }
        let result = psm::on_stack(base, size, || on_segment(base as usize, work));
        std::alloc::dealloc(base, layout);
        Ok(resume(result))
    }
}
