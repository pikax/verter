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

#[cfg(not(miri))]
fn stack_pointer() -> usize {
    psm::stack_pointer() as usize
}

/// Under Miri, which runs no assembly: the address of a local, as near the
/// stack pointer as the interpreter can say.
#[cfg(miri)]
fn stack_pointer() -> usize {
    let marker = 0u8;
    std::ptr::addr_of!(marker) as usize
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
#[track_caller]
pub fn with_stack<R>(
    needed: usize,
    purpose: Reservation,
    work: impl FnOnce() -> R,
) -> Result<R, StackUnavailable> {
    #[cfg(any(test, feature = "stack-fault-injection"))]
    let forced = faults::forcing(purpose);
    #[cfg(not(any(test, feature = "stack-fault-injection")))]
    let forced = false;
    if !forced && remaining().is_some_and(|left| left >= needed) {
        return Ok(work());
    }
    // A forced purpose takes no lease at all, one holding a region included:
    // the lease's region would run the work with no reservation of its own,
    // so a fault injected at [`Region::reserve`] would not be this call's to
    // refuse, and the armed fault would reach the next reservation instead.
    let lease = if forced { None } else { LEASE.with(Cell::get) };
    match lease {
        Some(lease) if lease.bytes >= needed => {
            match lease.region {
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
                    _ => Ok(reserve_past_lease(needed, purpose)?.run(work)),
                },
            }
        }
        _ => Ok(reserve_past_lease(needed, purpose)?.run(work)),
    }
}

/// Reserve a region for work no lease covers: a parse, or a walk outside
/// its operation's lease (which test builds record by its call site,
/// [`faults::take_unleased_walks`]).
#[track_caller]
fn reserve_past_lease(needed: usize, purpose: Reservation) -> Result<Region, StackUnavailable> {
    #[cfg(any(test, feature = "stack-fault-injection"))]
    if purpose == Reservation::Walk {
        faults::unleased_walk(std::panic::Location::caller());
    }
    Region::reserve(needed, purpose)
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
    #[cfg(any(test, feature = "stack-fault-injection"))]
    let forced = faults::forcing(Reservation::Lease);
    #[cfg(not(any(test, feature = "stack-fault-injection")))]
    let forced = false;
    if !forced
        && LEASE
            .with(Cell::get)
            .is_some_and(|lease| lease.bytes >= needed)
    {
        return Ok(operation());
    }
    let region = if !forced && remaining().is_some_and(|left| left >= needed) {
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

/// Whether an operation records its refusals on this thread
/// ([`refusals_within`]).
#[cfg(any(test, feature = "stack-fault-injection"))]
pub(super) fn recording() -> bool {
    REFUSALS.with(Cell::get).is_some()
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

    /// The purposes whose reservations are forced onto a region, however
    /// large the stack the thread runs on is: the guard
    /// [`force_reservations`] returns, one entry per nested force.
    static FORCED: Mutex<Vec<Reservation>> = Mutex::new(Vec::new());

    /// Make every reservation of each purpose in `purposes` take the region
    /// path, however much stack the thread that makes it runs on: the parse
    /// length shortcut, the thread's own stack, and the walk-stack lease's
    /// are all bypassed — a lease holding a region included, for the lease's
    /// region would run the work with no reservation of its own to refuse —
    /// so a fault injected at [`Region::reserve`] fires wherever the
    /// reservation is made, a scheduler worker's included.
    /// The force is scoped, like [`fail_reservations_needing`], to the
    /// process and by purpose, so it cannot reach another test's work
    /// under another purpose; the returned guard restores the forcings in
    /// force when it was taken, on drop and on unwind alike.
    pub fn force_reservations(purposes: &[Reservation]) -> ForcedRegions {
        FORCED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .extend_from_slice(purposes);
        ForcedRegions(purposes.to_vec())
    }

    /// Whether `purpose`'s reservations are forced onto a region.
    pub(in crate::oxc_parse) fn forcing(purpose: Reservation) -> bool {
        FORCED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains(&purpose)
    }

    /// The forcings [`force_reservations`] established, restored on drop.
    pub struct ForcedRegions(Vec<Reservation>);

    impl Drop for ForcedRegions {
        fn drop(&mut self) {
            let mut forced = FORCED
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            for purpose in self.0.drain(..) {
                if let Some(at) = forced.iter().position(|held| *held == purpose) {
                    forced.remove(at);
                }
            }
        }
    }

    /// The call sites of the walks, on any thread, whose stack refusal no
    /// operation would report: a walk no walk-stack lease covered, which can
    /// reserve a region of its own, and a leased walk that could reserve
    /// while no operation records its refusals.
    static UNLEASED: Mutex<Vec<&'static std::panic::Location<'static>>> = Mutex::new(Vec::new());

    pub(in crate::oxc_parse) fn unleased_walk(at: &'static std::panic::Location<'static>) {
        let mut unleased = UNLEASED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !unleased.contains(&at) {
            unleased.push(at);
        }
    }

    /// The call sites (`file:line`) of the walks, on any thread, whose stack
    /// refusal no operation would report, since the last call.
    pub fn take_unleased_walks() -> Vec<String> {
        let mut unleased = UNLEASED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut sites: Vec<String> = unleased
            .drain(..)
            .map(|at| format!("{}:{}", at.file().replace('\\', "/"), at.line()))
            .collect();
        sites.sort();
        sites
    }

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

    /// The size [`fail_reservations_here_after`] takes to match a reservation
    /// of any size.
    pub const ANY_SIZE: usize = usize::MAX;

    /// The reservations this thread made so far for `purpose`, of any size.
    pub fn reservations_here_of(purpose: Reservation) -> usize {
        MADE_HERE.with(|made| {
            made.borrow()
                .iter()
                .filter(|&&(held, _, _)| held == purpose)
                .map(|&(_, _, count)| count)
                .sum()
        })
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
            Some((held, bytes, skip, fail))
                if held == purpose && (bytes == needed || bytes == ANY_SIZE) =>
            {
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

/// The slot a Windows region's owner and its fiber hand work through: the
/// job the fiber runs next, and the fiber it switches back to.
///
/// Safety argument. The slot is heap memory the handoff owns as a raw
/// pointer (`Box::into_raw`, freed by `Drop`), never as a `Box` or a
/// reference, so moving the handoff does not re-assert unique ownership over
/// memory the fiber still points into. The slot sits in an `UnsafeCell`, and
/// both sides reach it only through `UnsafeCell::raw_get` and raw-pointer
/// reads and writes: no `&Slot` or `&mut Slot` is ever created, so no
/// reference's aliasing guarantee covers memory the other side writes. The
/// two sides never run at once: the owner writes the slot, then switches to
/// the fiber and runs nothing until the fiber switches back; the fiber reads
/// and clears the job, runs it, reads the parent, and switches back. Every
/// access is therefore ordered by a switch on one thread. The owner holds
/// only `&self` of the region while it writes: the writes go through the
/// cell, which permits mutation behind a shared reference.
#[cfg(any(windows, test))]
mod handoff {
    use std::cell::UnsafeCell;
    use std::ffi::c_void;
    use std::ptr::NonNull;

    struct Slot {
        job: Option<*mut dyn FnMut(usize)>,
        parent: *mut c_void,
    }

    /// The slot, owned as a raw pointer for as long as the handoff lives.
    pub(super) struct Handoff {
        cell: NonNull<UnsafeCell<Slot>>,
    }

    impl Handoff {
        pub(super) fn new() -> Handoff {
            let slot = Box::new(UnsafeCell::new(Slot {
                job: None,
                parent: std::ptr::null_mut(),
            }));
            Handoff {
                // SAFETY: `Box::into_raw` never returns null.
                cell: unsafe { NonNull::new_unchecked(Box::into_raw(slot)) },
            }
        }

        /// The pointer the fiber is created with, valid until the handoff
        /// drops.
        pub(super) fn fiber_data(&self) -> *mut c_void {
            self.cell.as_ptr().cast()
        }

        /// Post `job` for the fiber to run and `parent` for it to switch
        /// back to.
        ///
        /// # Safety
        ///
        /// The fiber is suspended, and stays suspended until the caller
        /// switches to it; `job` stays valid until the fiber switches back.
        pub(super) unsafe fn post(&self, job: *mut dyn FnMut(usize), parent: *mut c_void) {
            let slot = UnsafeCell::raw_get(self.cell.as_ptr());
            // SAFETY: the slot is alive (owned by `self`), and the fiber,
            // the only other side, is suspended.
            unsafe {
                std::ptr::write(&raw mut (*slot).job, Some(job));
                std::ptr::write(&raw mut (*slot).parent, parent);
            }
        }

        /// On the fiber: take the job posted for it, clearing the slot.
        ///
        /// # Safety
        ///
        /// `data` is a live handoff's [`Self::fiber_data`], and its owner is
        /// suspended in a switch to this fiber.
        pub(super) unsafe fn take_job(data: *mut c_void) -> Option<*mut dyn FnMut(usize)> {
            let slot = UnsafeCell::raw_get(data.cast::<UnsafeCell<Slot>>());
            // SAFETY: as the caller guarantees; the owner reads nothing
            // until this fiber switches back.
            unsafe { std::ptr::replace(&raw mut (*slot).job, None) }
        }

        /// On the fiber: the fiber to switch back to.
        ///
        /// # Safety
        ///
        /// As [`Self::take_job`].
        pub(super) unsafe fn parent(data: *mut c_void) -> *mut c_void {
            let slot = UnsafeCell::raw_get(data.cast::<UnsafeCell<Slot>>());
            // SAFETY: as the caller guarantees.
            unsafe { std::ptr::read(&raw const (*slot).parent) }
        }
    }

    impl Drop for Handoff {
        fn drop(&mut self) {
            // SAFETY: the pointer came from `Box::into_raw` in `new` and is
            // freed once, here; the fiber that read it is deleted first.
            drop(unsafe { Box::from_raw(self.cell.as_ptr()) });
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::handoff::Handoff;
    use std::ffi::c_void;
    use windows_sys::Win32::System::Threading::{
        ConvertFiberToThread, ConvertThreadToFiber, CreateFiberEx, DeleteFiber, IsThreadAFiber,
        SwitchToFiber,
    };

    /// A fiber that runs each job it is switched to, then switches back.
    /// The region's `Drop` deletes the fiber before its `handoff` field
    /// drops and frees the slot the fiber points into.
    pub(super) struct Region {
        fiber: *mut c_void,
        handoff: Handoff,
    }

    unsafe extern "system" fn serve(data: *mut c_void) {
        let limit = fiber_stack_limit();
        loop {
            // SAFETY: `data` is the region's handoff, alive while the fiber
            // is; `run` posts the job and parent before switching here and
            // touches nothing until this fiber switches back.
            unsafe {
                if let Some(job) = Handoff::take_job(data) {
                    (*job)(limit);
                }
                SwitchToFiber(Handoff::parent(data));
            }
        }
    }

    impl Region {
        pub(super) fn reserve(needed: usize) -> Option<Region> {
            // The system's guard pages and stack-overflow guarantee come out
            // of the reservation.
            let reserve = needed.checked_add(fiber_overhead())?;
            let handoff = Handoff::new();
            // SAFETY: the fiber runs `serve` over the handoff's slot, which
            // the region owns for as long as the fiber exists.
            let fiber = unsafe {
                CreateFiberEx(
                    super::INITIAL_COMMIT_BYTES,
                    reserve,
                    0,
                    Some(serve),
                    handoff.fiber_data(),
                )
            };
            (!fiber.is_null()).then(|| Region { fiber, handoff })
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
                // Erase the job's lifetime: the fiber runs it before this
                // call returns.
                let job: *mut (dyn FnMut(usize) + '_) = job;
                let job = std::mem::transmute::<
                    *mut (dyn FnMut(usize) + '_),
                    *mut (dyn FnMut(usize) + 'static),
                >(job);
                // The fiber is suspended in `serve` (it runs only while this
                // thread switches to it, and has switched back), and `job`
                // outlives the switch.
                self.handoff.post(job, parent);
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

/// The handoff between a Windows region's owner and its fiber, the switch
/// between them played by a direct call: run under Miri, which checks the
/// owner's writes through a shared reference and the fiber's reads through
/// its raw pointer against the aliasing model (`cargo +nightly miri test -p
/// verter_parser --lib -- handoff`).
#[cfg(test)]
mod handoff_tests {
    use super::handoff::Handoff;
    use std::ffi::c_void;

    /// The fiber's side of one switch: take the job, run it, answer the
    /// parent it switches back to.
    fn serve_once(data: *mut c_void, limit: usize) -> *mut c_void {
        // SAFETY: `data` is a live handoff's, and its owner posted before
        // this call and touches nothing during it.
        unsafe {
            if let Some(job) = Handoff::take_job(data) {
                (*job)(limit);
            }
            Handoff::parent(data)
        }
    }

    #[test]
    fn a_posted_job_runs_once_and_answers_its_parent() {
        let handoff = Handoff::new();
        let data = handoff.fiber_data();
        let mut ran = Vec::new();
        let parent = std::ptr::without_provenance_mut::<c_void>(0x10);
        for round in 0..3 {
            let mut job = |limit: usize| ran.push((round, limit));
            let job: *mut (dyn FnMut(usize) + '_) = &mut job;
            // SAFETY: the job outlives the "switch" below, and nothing
            // serves the handoff while it is posted to.
            unsafe {
                handoff.post(
                    std::mem::transmute::<
                        *mut (dyn FnMut(usize) + '_),
                        *mut (dyn FnMut(usize) + 'static),
                    >(job),
                    parent,
                )
            };
            assert_eq!(serve_once(data, 7 + round), parent);
            // A switch with no job posted runs nothing.
            assert_eq!(serve_once(data, 0), parent);
        }
        assert_eq!(ran, [(0, 7), (1, 8), (2, 9)]);
    }

    /// The fiber keeps the pointer it was created with while its owner
    /// moves: the slot is not the handoff's to re-borrow on a move.
    #[test]
    fn the_fiber_pointer_survives_the_owner_moving() {
        let handoff = Handoff::new();
        let data = handoff.fiber_data();
        let moved = vec![handoff];
        let mut count = 0;
        let mut job = |_: usize| count += 1;
        let job: *mut (dyn FnMut(usize) + '_) = &mut job;
        // SAFETY: as above.
        unsafe {
            moved[0].post(
                std::mem::transmute::<
                    *mut (dyn FnMut(usize) + '_),
                    *mut (dyn FnMut(usize) + 'static),
                >(job),
                std::ptr::null_mut(),
            )
        };
        serve_once(data, 0);
        drop(moved);
        assert_eq!(count, 1);
    }
}
