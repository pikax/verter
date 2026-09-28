//! The stack a parse, or a walk of oxc's, runs on.
//!
//! Work runs in place when the current stack has the bytes it can need,
//! and otherwise on a stack [`Region`] reserved for it: on Windows a fiber
//! whose stack reserves the size and commits [`INITIAL_COMMIT_BYTES`] of it
//! (the system commits the rest page by page as the work reaches it), on
//! Unix an anonymous mapping the system backs page by page, and on wasm32 a
//! heap allocation holding the module's shadow stack. A region is sized
//! from the work's nesting, so a deep source reserves address space in
//! proportion to its depth but commits memory in proportion to the stack
//! its work touches.
//!
//! Reserving is the only step that can fail, and it fails as a
//! [`StackUnavailable`], never a panic. An operation pays for it once, at
//! its boundary, by holding a walk-stack lease ([`with_walk_stack_lease`]):
//! the lease reserves the region the operation's walks can need before
//! any of them begins, and every walk inside the lease that needs no more
//! than the lease holds runs on that region, switched to without reserving
//! again, or in place when it already runs on it.

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

/// The stack a Windows region commits when it is reserved; the system
/// commits more, a page at a time, as the work on it grows the stack.
#[cfg(windows)]
pub const INITIAL_COMMIT_BYTES: usize = 64 * 1024;

/// The walk-stack lease an operation holds: the bytes it covers, and the
/// region it reserved for them (`None` where the thread's own stack had
/// them when the lease began).
#[derive(Clone, Copy)]
struct Lease {
    bytes: usize,
    region: Option<*const Region>,
}

thread_local! {
    /// The lowest usable address of the region this thread runs on, while
    /// it runs on one; `None` on the thread's own stack.
    static SEGMENT_LIMIT: Cell<Option<usize>> = const { Cell::new(None) };
    /// The region this thread runs on, while it runs on one.
    static ON_REGION: Cell<*const Region> = const { Cell::new(std::ptr::null()) };
    /// The innermost walk-stack lease this thread holds.
    static LEASE: Cell<Option<Lease>> = const { Cell::new(None) };
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

/// Run `work`, a parse or a walk, with at least `needed` bytes of stack: in
/// place when the stack this thread runs on has them; on the lease's
/// region, without reserving, when the thread holds a lease covering them;
/// on a region of `needed` bytes reserved for it otherwise.
pub fn with_stack<R>(
    needed: usize,
    purpose: Reservation,
    work: impl FnOnce() -> R,
) -> Result<R, StackUnavailable> {
    if remaining().is_some_and(|left| left >= needed) {
        return Ok(work());
    }
    match LEASE.with(Cell::get) {
        Some(lease) if lease.bytes >= needed => match lease.region {
            // On the lease's region already, or on the stack the lease found
            // large enough: the lease's size covers the walk.
            None => Ok(work()),
            Some(region) if ON_REGION.with(Cell::get) == region => Ok(work()),
            // SAFETY: a lease's region lives until the lease ends, and the
            // lease is this thread's, active for the whole of this call.
            Some(region) => match unsafe { &*region } {
                region if !region.busy.get() => Ok(region.run(work)),
                // Work on the region is suspended under a region reserved
                // past the lease: this walk gets one of its own.
                _ => Ok(Region::reserve(needed, purpose)?.run(work)),
            },
        },
        _ => Ok(Region::reserve(needed, purpose)?.run(work)),
    }
}

/// Run `operation` holding a walk-stack lease of `needed` bytes: the
/// region every walk inside it needing no more runs on, reserved before
/// `operation` begins. When the thread already holds a lease covering
/// `needed` bytes, or its own stack has them, nothing is reserved. When the
/// region cannot be reserved, `operation` does not run.
pub fn with_walk_stack_lease<R>(
    needed: usize,
    operation: impl FnOnce() -> R,
) -> Result<R, StackUnavailable> {
    if LEASE
        .with(Cell::get)
        .is_some_and(|lease| lease.bytes >= needed)
    {
        return Ok(operation());
    }
    let region = if remaining().is_some_and(|left| left >= needed) {
        None
    } else {
        match Region::reserve(needed, Reservation::Lease) {
            Ok(region) => Some(region),
            Err(unavailable) => return Err(record_refusal(unavailable)),
        }
    };
    let _lease = LeaseScope::enter(Lease {
        bytes: needed,
        region: region.as_ref().map(|region| region as *const Region),
    });
    Ok(operation())
}

thread_local! {
    /// While an operation records its refusals ([`refusals_within`]), the
    /// first stack refusal made inside it on this thread; `None` while no
    /// operation records.
    static REFUSALS: Cell<Option<Option<StackUnavailable>>> = const { Cell::new(None) };
}

/// Run `operation`, returning with its result the first stack refusal made
/// inside it on this thread: a parse not run, or a walk-stack lease not
/// reserved. An operation that made one is incomplete whatever it
/// returns, and so is every operation enclosing it that records its
/// refusals.
pub fn refusals_within<R>(operation: impl FnOnce() -> R) -> (R, Option<StackUnavailable>) {
    /// Restores the enclosing operation's record, on return and on unwind
    /// alike, carrying this operation's refusal into it.
    struct Recording(Option<Option<StackUnavailable>>);
    impl Drop for Recording {
        fn drop(&mut self) {
            let inner = REFUSALS.with(|refusals| refusals.replace(self.0)).flatten();
            if let (Some(refused), Some(None)) = (inner, self.0) {
                REFUSALS.with(|refusals| refusals.set(Some(Some(refused))));
            }
        }
    }
    let recording = Recording(REFUSALS.with(|refusals| refusals.replace(Some(None))));
    let result = operation();
    let refused = REFUSALS.with(Cell::get).flatten();
    drop(recording);
    (result, refused)
}

/// Record `unavailable` for the operation recording its refusals on this
/// thread, if one is; returns it.
pub(super) fn record_refusal(unavailable: StackUnavailable) -> StackUnavailable {
    REFUSALS.with(|refusals| {
        if refusals.get() == Some(None) {
            refusals.set(Some(Some(unavailable)));
        }
    });
    unavailable
}

/// Whether the thread holds a walk-stack lease covering `needed` bytes.
pub fn lease_covers(needed: usize) -> bool {
    LEASE
        .with(Cell::get)
        .is_some_and(|lease| lease.bytes >= needed)
}

/// Holds a lease active until dropped, restoring the enclosing one then,
/// on return and on unwind alike.
struct LeaseScope(Option<Lease>);

impl LeaseScope {
    fn enter(lease: Lease) -> Self {
        Self(LEASE.with(|current| current.replace(Some(lease))))
    }
}

impl Drop for LeaseScope {
    fn drop(&mut self) {
        LEASE.with(|current| current.set(self.0));
    }
}

/// Run `work` on the current region's stack under `limit`, restoring the
/// enclosing stack's afterwards; a panic in `work` is carried out to the
/// caller of [`Region::run`], never unwound across the stack switch.
fn on_segment<R>(
    region: *const Region,
    limit: usize,
    work: impl FnOnce() -> R,
) -> std::thread::Result<R> {
    let outer_limit = SEGMENT_LIMIT.with(|segment| segment.replace(Some(limit)));
    let outer_region = ON_REGION.with(|on| on.replace(region));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work));
    ON_REGION.with(|on| on.set(outer_region));
    SEGMENT_LIMIT.with(|segment| segment.set(outer_limit));
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

/// What a stack region is reserved for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reservation {
    /// A parse whose source nests deeper than the thread's stack.
    Parse,
    /// An operation's walk-stack lease.
    Lease,
    /// A walk no lease covers.
    Walk,
}

/// Reservation counting and fault injection at the one step that can fail:
/// tests make reservations fail and count them, without exhausting the
/// machine. A thread's own next reservations fail on
/// [`faults::fail_next_reservations`]; a reservation on any thread (a
/// scheduler worker's) fails on [`faults::fail_reservations_needing`] when
/// its purpose and size match.
#[cfg(any(test, feature = "stack-fault-injection"))]
pub mod faults {
    use std::cell::Cell;
    use std::sync::Mutex;

    pub use super::Reservation;

    thread_local! {
        static FAILING: Cell<usize> = const { Cell::new(0) };
        static RESERVED: Cell<usize> = const { Cell::new(0) };
        /// This thread's own reservations to fail, keyed by purpose and bytes:
        /// the matching ones still to let through first, then the ones to fail.
        static HERE: Cell<Option<(Reservation, usize, usize, usize)>> = const { Cell::new(None) };
        /// This thread's own reservations so far, by purpose and bytes.
        static MADE_HERE: std::cell::RefCell<Vec<(Reservation, usize, usize)>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }

    /// Reservations on any thread to fail, keyed by purpose and bytes.
    struct Target {
        purpose: Reservation,
        needed: usize,
        /// Matching reservations still to let through first.
        skip: usize,
        /// Matching reservations then to fail.
        fail: usize,
    }

    static TARGETED: Mutex<Vec<Target>> = Mutex::new(Vec::new());

    /// Every reservation on any thread so far, by purpose and bytes.
    static MADE: Mutex<Vec<(Reservation, usize, usize)>> = Mutex::new(Vec::new());

    /// Make the next `count` reservations on this thread fail.
    pub fn fail_next_reservations(count: usize) {
        FAILING.with(|failing| failing.set(count));
    }

    /// Make the next `count` reservations for `purpose` of exactly `needed`
    /// bytes that this thread makes fail; another thread's, of the same
    /// purpose and size, go through.
    pub fn fail_reservations_here(purpose: Reservation, needed: usize, count: usize) {
        fail_reservations_here_after(purpose, needed, 0, count);
    }

    /// Let the next `skip` reservations for `purpose` of exactly `needed`
    /// bytes that this thread makes through, then make the `count` after
    /// them fail; another thread's go through.
    pub fn fail_reservations_here_after(
        purpose: Reservation,
        needed: usize,
        skip: usize,
        count: usize,
    ) {
        HERE.with(|here| here.set((count > 0).then_some((purpose, needed, skip, count))));
    }

    /// The reservations this thread made so far for `purpose` of exactly
    /// `needed` bytes.
    pub fn reservations_here(purpose: Reservation, needed: usize) -> usize {
        MADE_HERE.with(|made| {
            made.borrow()
                .iter()
                .find(|&&(held, bytes, _)| (held, bytes) == (purpose, needed))
                .map_or(0, |&(_, _, count)| count)
        })
    }

    /// Make the next `count` reservations for `purpose` of exactly `needed`
    /// bytes fail, on whichever thread makes them.
    pub fn fail_reservations_needing(purpose: Reservation, needed: usize, count: usize) {
        fail_reservations_needing_after(purpose, needed, 0, count);
    }

    /// Let the next `skip` reservations for `purpose` of exactly `needed`
    /// bytes through, then make the `count` after them fail, on whichever
    /// thread makes them.
    pub fn fail_reservations_needing_after(
        purpose: Reservation,
        needed: usize,
        skip: usize,
        count: usize,
    ) {
        let mut targeted = TARGETED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        targeted.retain(|target| (target.purpose, target.needed) != (purpose, needed));
        if count > 0 {
            targeted.push(Target {
                purpose,
                needed,
                skip,
                fail: count,
            });
        }
    }

    /// The reservations made so far, on any thread, for `purpose` of
    /// exactly `needed` bytes.
    pub fn reservations_needing(purpose: Reservation, needed: usize) -> usize {
        MADE.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .find(|&&(held, bytes, _)| (held, bytes) == (purpose, needed))
            .map_or(0, |&(_, _, count)| count)
    }

    /// The reservations this thread attempted since the last call, failed
    /// ones included.
    pub fn take_reservations() -> usize {
        RESERVED.with(|reserved| reserved.replace(0))
    }

    /// Count one reservation; whether it is to fail.
    pub(super) fn reserving(needed: usize, purpose: Reservation) -> bool {
        RESERVED.with(|reserved| reserved.set(reserved.get() + 1));
        {
            let mut made = MADE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            match made
                .iter_mut()
                .find(|(held, bytes, _)| (*held, *bytes) == (purpose, needed))
            {
                Some(entry) => entry.2 += 1,
                None => made.push((purpose, needed, 1)),
            }
        }
        let thread_fails = FAILING.with(|failing| {
            let left = failing.get();
            failing.set(left.saturating_sub(1));
            left > 0
        });
        if thread_fails {
            return true;
        }
        MADE_HERE.with(|made| {
            let mut made = made.borrow_mut();
            match made
                .iter_mut()
                .find(|(held, bytes, _)| (*held, *bytes) == (purpose, needed))
            {
                Some(entry) => entry.2 += 1,
                None => made.push((purpose, needed, 1)),
            }
        });
        let here_fails = HERE.with(|here| match here.get() {
            Some((held, bytes, skip, fail)) if (held, bytes) == (purpose, needed) => {
                if skip > 0 {
                    here.set(Some((held, bytes, skip - 1, fail)));
                    return false;
                }
                here.set((fail > 1).then_some((held, bytes, 0, fail - 1)));
                true
            }
            _ => false,
        });
        if here_fails {
            return true;
        }
        let mut targeted = TARGETED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(at) = targeted
            .iter()
            .position(|target| (target.purpose, target.needed) == (purpose, needed))
        else {
            return false;
        };
        let target = &mut targeted[at];
        if target.skip > 0 {
            target.skip -= 1;
            return false;
        }
        target.fail -= 1;
        if target.fail == 0 {
            targeted.remove(at);
        }
        true
    }
}

/// A stack reserved for work to run on, any number of times, until it is
/// dropped.
pub(crate) struct Region {
    platform: platform::Region,
    /// Work runs on the region, possibly suspended under another.
    busy: Cell<bool>,
}

impl Region {
    /// Reserve a region of at least `needed` bytes of stack.
    pub(crate) fn reserve(needed: usize, purpose: Reservation) -> Result<Region, StackUnavailable> {
        #[cfg(any(test, feature = "stack-fault-injection"))]
        if faults::reserving(needed, purpose) {
            return Err(StackUnavailable { needed });
        }
        #[cfg(not(any(test, feature = "stack-fault-injection")))]
        let _ = purpose;
        platform::Region::reserve(needed)
            .map(|platform| Region {
                platform,
                busy: Cell::new(false),
            })
            .ok_or(StackUnavailable { needed })
    }

    /// Run `work` on this region's stack. It cannot fail: the region is
    /// already reserved.
    pub(crate) fn run<R>(&self, work: impl FnOnce() -> R) -> R {
        let region: *const Region = self;
        let mut work = Some(work);
        let mut result = None;
        self.busy.set(true);
        self.platform.run(&mut |limit| {
            let work = work.take().expect("a region runs its work once per call");
            result = Some(on_segment(region, limit, work));
        });
        self.busy.set(false);
        resume(result.expect("the region ran its work"))
    }
}

#[cfg(windows)]
mod platform {
    use std::ffi::c_void;
    use windows_sys::Win32::System::Threading::{
        ConvertFiberToThread, ConvertThreadToFiber, CreateFiberEx, DeleteFiber, IsThreadAFiber,
        SwitchToFiber,
    };

    /// What the region's fiber runs next, and the fiber to switch back to.
    struct Slot {
        job: Option<*mut dyn FnMut(usize)>,
        parent: *mut c_void,
    }

    /// A fiber that runs each job it is switched to, then switches back.
    pub(super) struct Region {
        fiber: *mut c_void,
        slot: Box<Slot>,
    }

    unsafe extern "system" fn serve(data: *mut c_void) {
        let slot = data.cast::<Slot>();
        let limit = fiber_stack_limit();
        loop {
            // SAFETY: `slot` is the region's, alive while the fiber is;
            // `run` sets the job and parent before switching here and reads
            // nothing until this fiber switches back.
            unsafe {
                if let Some(job) = (*slot).job.take() {
                    (*job)(limit);
                }
                SwitchToFiber((*slot).parent);
            }
        }
    }

    impl Region {
        pub(super) fn reserve(needed: usize) -> Option<Region> {
            // The system's guard pages and stack-overflow guarantee come out
            // of the reservation.
            let reserve = needed.checked_add(fiber_overhead())?;
            let mut slot = Box::new(Slot {
                job: None,
                parent: std::ptr::null_mut(),
            });
            // SAFETY: the fiber runs `serve` over `slot`, which the region
            // owns for as long as the fiber exists.
            let fiber = unsafe {
                CreateFiberEx(
                    super::INITIAL_COMMIT_BYTES,
                    reserve,
                    0,
                    Some(serve),
                    (&raw mut *slot).cast(),
                )
            };
            (!fiber.is_null()).then(|| Region { fiber, slot })
        }

        pub(super) fn run(&self, job: &mut dyn FnMut(usize)) {
            // SAFETY: plain Win32 fiber calls. The thread becomes a fiber for
            // the switch when it is not one; the region's fiber runs `job`,
            // which outlives the switch, then switches back to `parent`.
            unsafe {
                let converted = IsThreadAFiber() == 0;
                let parent = if converted {
                    ConvertThreadToFiber(std::ptr::null())
                } else {
                    current_fiber()
                };
                // Converting cannot fail but for memory for the fiber's
                // data; without it the job runs in place, as the stack the
                // thread has is all it can have.
                if parent.is_null() {
                    job(super::stack_pointer());
                    return;
                }
                let slot = (&raw const *self.slot).cast_mut();
                // Erase the job's lifetime: the fiber runs it before this
                // call returns.
                let job: *mut (dyn FnMut(usize) + '_) = job;
                (*slot).job = Some(std::mem::transmute::<
                    *mut (dyn FnMut(usize) + '_),
                    *mut (dyn FnMut(usize) + 'static),
                >(job));
                (*slot).parent = parent;
                SwitchToFiber(self.fiber);
                if converted {
                    ConvertFiberToThread();
                }
            }
        }
    }

    impl Drop for Region {
        fn drop(&mut self) {
            // SAFETY: the fiber is suspended in `serve`, never running while
            // its region is dropped.
            unsafe { DeleteFiber(self.fiber) };
        }
    }

    /// The fiber this thread runs as: `GetCurrentFiber`, which Windows
    /// defines inline, reading the thread information block's `FiberData`.
    unsafe fn current_fiber() -> *mut c_void {
        let fiber: *mut c_void;
        // SAFETY: the thread information block is mapped for every thread,
        // at the segment register (x86) or register (aarch64) Windows keeps
        // it in.
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

    /// The bytes a Windows stack keeps from its reservation: the guarantee
    /// the system holds back to handle a stack overflow, and a guard page.
    fn fiber_overhead() -> usize {
        use windows_sys::Win32::System::Threading::SetThreadStackGuarantee;
        let minimum = if cfg!(target_pointer_width = "32") {
            0x1000
        } else {
            0x2000
        };
        let mut guarantee = 0u32;
        // SAFETY: a zero guarantee asks for the current one without
        // changing it.
        let read = unsafe { SetThreadStackGuarantee(&mut guarantee) } != 0;
        let guarantee = if read { guarantee as usize } else { 0 };
        guarantee.max(minimum) + 2 * 0x1000
    }

    /// The lowest address the current fiber's stack can use: the base of
    /// its reservation above what [`fiber_overhead`] holds back.
    fn fiber_stack_limit() -> usize {
        use windows_sys::Win32::System::Memory::{VirtualQuery, MEMORY_BASIC_INFORMATION};
        let mut info = std::mem::MaybeUninit::<MEMORY_BASIC_INFORMATION>::uninit();
        // SAFETY: `info` is writable and as large as the call is told.
        let read = unsafe {
            VirtualQuery(
                super::stack_pointer() as *const _,
                info.as_mut_ptr(),
                std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        if read == 0 {
            // The stack's base is unknown: nothing on it counts as spare.
            return super::stack_pointer();
        }
        // SAFETY: `VirtualQuery` filled `info`.
        let base = unsafe { info.assume_init() }.AllocationBase as usize;
        base + fiber_overhead()
    }
}

#[cfg(all(unix, not(target_arch = "wasm32")))]
mod platform {
    /// An anonymous mapping with a guard page below the stack and one
    /// above it.
    pub(super) struct Region {
        mapping: *mut libc::c_void,
        mapped: usize,
        base: *mut u8,
        usable: usize,
    }

    impl Region {
        pub(super) fn reserve(needed: usize) -> Option<Region> {
            // SAFETY: `sysconf` has no preconditions.
            let page = usize::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) })
                .ok()
                .filter(|page| page.is_power_of_two())
                .unwrap_or(4096);
            let usable = needed.checked_add(page - 1)? & !(page - 1);
            let mapped = usable.checked_add(2 * page)?;
            #[cfg(any(target_os = "linux", target_os = "android"))]
            let flags = libc::MAP_PRIVATE | libc::MAP_ANON | libc::MAP_NORESERVE;
            #[cfg(not(any(target_os = "linux", target_os = "android")))]
            let flags = libc::MAP_PRIVATE | libc::MAP_ANON;
            // SAFETY: an anonymous private mapping of `mapped` bytes, which
            // the region unmaps when dropped; work runs on its middle pages.
            unsafe {
                let mapping =
                    libc::mmap(std::ptr::null_mut(), mapped, libc::PROT_NONE, flags, -1, 0);
                if mapping == libc::MAP_FAILED {
                    return None;
                }
                let base = mapping.cast::<u8>().add(page);
                if libc::mprotect(base.cast(), usable, libc::PROT_READ | libc::PROT_WRITE) != 0 {
                    libc::munmap(mapping, mapped);
                    return None;
                }
                Some(Region {
                    mapping,
                    mapped,
                    base,
                    usable,
                })
            }
        }

        pub(super) fn run(&self, job: &mut dyn FnMut(usize)) {
            let limit = self.base as usize;
            // SAFETY: the region's middle pages are mapped read-write for as
            // long as the region lives, and nothing else runs on them.
            unsafe { psm::on_stack(self.base, self.usable, || job(limit)) }
        }
    }

    impl Drop for Region {
        fn drop(&mut self) {
            // SAFETY: the mapping is the region's, and no work runs on it.
            unsafe { libc::munmap(self.mapping, self.mapped) };
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod platform {
    /// A heap allocation holding the module's shadow stack.
    pub(super) struct Region {
        base: *mut u8,
        layout: std::alloc::Layout,
    }

    impl Region {
        pub(super) fn reserve(needed: usize) -> Option<Region> {
            let size = needed.checked_add(15)? & !15;
            let layout = std::alloc::Layout::from_size_align(size, 16).ok()?;
            // SAFETY: `layout` has a non-zero size; the region frees the
            // allocation when dropped.
            let base = unsafe { std::alloc::alloc(layout) };
            (!base.is_null()).then_some(Region { base, layout })
        }

        pub(super) fn run(&self, job: &mut dyn FnMut(usize)) {
            let limit = self.base as usize;
            // SAFETY: the allocation is the region's for as long as it
            // lives, and nothing else runs on it.
            unsafe { psm::on_stack(self.base, self.layout.size(), || job(limit)) }
        }
    }

    impl Drop for Region {
        fn drop(&mut self) {
            // SAFETY: the allocation is the region's, and no work runs on it.
            unsafe { std::alloc::dealloc(self.base, self.layout) };
        }
    }
}
