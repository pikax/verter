//! Compile-time availability and execution vocabulary for optional capture.
//!
//! Availability describes this binary, independently of whether a request asks
//! for capture. Existing instrumentation is not migrated by this module.

/// Whether this binary can collect optional semantic observations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureAvailability {
    /// Optional capture is compiled out; missing metrics are not zero metrics.
    Unavailable,
    /// The binary was built with `semantic-observe`.
    Available,
}

impl CaptureAvailability {
    /// Report the compile-time capability, independently of the build profile.
    pub const fn compiled() -> Self {
        #[cfg(feature = "semantic-observe")]
        {
            Self::Available
        }
        #[cfg(not(feature = "semantic-observe"))]
        {
            Self::Unavailable
        }
    }
}

/// Concrete execution modes carried by optional capture owners.
///
/// A root entry selects its mode once with [`ObserveMode::select`] (the
/// session's `capture::select_mode`); the request context carries the
/// choice, and every scheduler resumption and background job reinstalls that
/// context. An execution unit — a semantic dispatcher and the request
/// context it resolves through — adopts the carried choice once, as a type
/// ([`specialise!`](crate::specialise)): its hot paths are compiled for
/// [`Captured`] or [`Uncaptured`] and never test the mode per emission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObserveMode {
    /// Execute without optional observations.
    Uncaptured,
    /// Execute with optional observations.
    Captured,
}

impl ObserveMode {
    /// Select the mode for one root entry: captured only when this binary
    /// compiles capture in and the entry asks for it.
    #[must_use]
    pub const fn select(requested: bool) -> Self {
        if requested
            && matches!(
                CaptureAvailability::compiled(),
                CaptureAvailability::Available
            )
        {
            Self::Captured
        } else {
            Self::Uncaptured
        }
    }

    /// `true` for [`ObserveMode::Captured`].
    #[must_use]
    pub const fn is_captured(self) -> bool {
        matches!(self, Self::Captured)
    }

    /// The execution mode supported by this build before per-request selection.
    pub const fn compiled() -> Self {
        #[cfg(feature = "semantic-observe")]
        {
            Self::Captured
        }
        #[cfg(not(feature = "semantic-observe"))]
        {
            Self::Uncaptured
        }
    }
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::Captured {}
    impl Sealed for super::Uncaptured {}
}

/// An execution mode as a type: the code an execution unit runs is compiled
/// once per mode, so an emission site tests a constant, never a runtime flag
/// or a thread-local slot.
///
/// Sealed: exactly [`Captured`] and [`Uncaptured`] exist, so specialisation is
/// bounded to the two modes.
pub trait ObserveExec: sealed::Sealed + Copy + Default + Send + Sync + 'static {
    /// The runtime mode this type stands for.
    const MODE: ObserveMode;
    /// `true` only for [`Captured`] in a build with `semantic-observe`: the
    /// constant every typed emission branches on. A default build never
    /// selects [`Captured`] and compiles no captured emission either way.
    const CAPTURED: bool;
}

/// Captured execution: optional observations are recorded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Captured;

/// Uncaptured execution: optional observations compile to nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Uncaptured;

impl ObserveExec for Captured {
    const MODE: ObserveMode = ObserveMode::Captured;
    const CAPTURED: bool = cfg!(feature = "semantic-observe");
}

impl ObserveExec for Uncaptured {
    const MODE: ObserveMode = ObserveMode::Uncaptured;
    const CAPTURED: bool = false;
}

/// Run `body` compiled for the execution mode `mode` selects, with `$M`
/// naming that mode's type inside it:
/// `specialise!(mode => M, { run::<M>(...) })`.
///
/// The body is expanded once per mode, so everything it reaches through
/// `M` is the concrete captured or uncaptured execution; this one match on
/// the carried mode is the only test of it. Without `semantic-observe` only
/// the [`Uncaptured`] expansion exists and `mode` is not read.
#[cfg(feature = "semantic-observe")]
#[macro_export]
macro_rules! specialise {
    ($mode:expr => $M:ident, $body:block) => {
        match $mode {
            $crate::observe::ObserveMode::Captured => {
                type $M = $crate::observe::Captured;
                $body
            }
            $crate::observe::ObserveMode::Uncaptured => {
                type $M = $crate::observe::Uncaptured;
                $body
            }
        }
    };
}

/// Run `body` compiled for the execution mode `mode` selects. See the
/// `semantic-observe` form; this default build expands only the
/// [`Uncaptured`] body.
#[cfg(not(feature = "semantic-observe"))]
#[macro_export]
macro_rules! specialise {
    ($mode:expr => $M:ident, $body:block) => {{
        let _ = || $mode;
        type $M = $crate::observe::Uncaptured;
        $body
    }};
}

/// Collect a capture payload only when optional capture is compiled in.
///
/// `None` means unavailable capture, never a fabricated empty/zero payload.
/// Build the payload inside `collect`: a feature-disabled build never calls it.
/// This is a capture boundary, not an enabled check for semantic hot loops.
pub fn capture<T>(collect: impl FnOnce() -> T) -> Option<T> {
    #[cfg(feature = "semantic-observe")]
    {
        Some(collect())
    }
    #[cfg(not(feature = "semantic-observe"))]
    {
        let _ = collect;
        None
    }
}

/// Optional per-request counter: a relaxed `u64` atomic with
/// `semantic-observe`, a zero-size value whose writes compile to nothing
/// without it.
///
/// The reader exists only where the counter is kept, so a default build
/// cannot substitute zero for a measurement it never took.
#[derive(Debug, Default)]
pub struct ObservedU64 {
    #[cfg(feature = "semantic-observe")]
    value: std::sync::atomic::AtomicU64,
}

impl ObservedU64 {
    /// Add `amount`. A no-op without `semantic-observe`.
    #[inline(always)]
    pub fn fetch_add(&self, amount: u64, order: std::sync::atomic::Ordering) {
        #[cfg(feature = "semantic-observe")]
        self.value.fetch_add(amount, order);
        #[cfg(not(feature = "semantic-observe"))]
        let _ = (amount, order);
    }

    /// Overwrite the counter. A no-op without `semantic-observe`.
    #[inline(always)]
    pub fn store(&self, value: u64, order: std::sync::atomic::Ordering) {
        #[cfg(feature = "semantic-observe")]
        self.value.store(value, order);
        #[cfg(not(feature = "semantic-observe"))]
        let _ = (value, order);
    }

    /// Subtract `amount`. A no-op without `semantic-observe`.
    #[inline(always)]
    pub fn fetch_sub(&self, amount: u64, order: std::sync::atomic::Ordering) {
        #[cfg(feature = "semantic-observe")]
        self.value.fetch_sub(amount, order);
        #[cfg(not(feature = "semantic-observe"))]
        let _ = (amount, order);
    }

    /// Raise the counter to at least `value`. A no-op without
    /// `semantic-observe`.
    #[inline(always)]
    pub fn fetch_max(&self, value: u64, order: std::sync::atomic::Ordering) {
        #[cfg(feature = "semantic-observe")]
        self.value.fetch_max(value, order);
        #[cfg(not(feature = "semantic-observe"))]
        let _ = (value, order);
    }

    /// Read the counter. Exists only where the counter is kept.
    #[cfg(feature = "semantic-observe")]
    #[must_use]
    pub fn load(&self, order: std::sync::atomic::Ordering) -> u64 {
        self.value.load(order)
    }
}

/// Optional per-request `u32` counter or bit mask. See [`ObservedU64`].
#[derive(Debug, Default)]
pub struct ObservedU32 {
    #[cfg(feature = "semantic-observe")]
    value: std::sync::atomic::AtomicU32,
}

impl ObservedU32 {
    /// Add `amount`. A no-op without `semantic-observe`.
    #[inline(always)]
    pub fn fetch_add(&self, amount: u32, order: std::sync::atomic::Ordering) {
        #[cfg(feature = "semantic-observe")]
        self.value.fetch_add(amount, order);
        #[cfg(not(feature = "semantic-observe"))]
        let _ = (amount, order);
    }

    /// Set the bits of `mask`. A no-op without `semantic-observe`.
    #[inline(always)]
    pub fn fetch_or(&self, mask: u32, order: std::sync::atomic::Ordering) {
        #[cfg(feature = "semantic-observe")]
        self.value.fetch_or(mask, order);
        #[cfg(not(feature = "semantic-observe"))]
        let _ = (mask, order);
    }

    /// Read the value. Exists only where it is kept.
    #[cfg(feature = "semantic-observe")]
    #[must_use]
    pub fn load(&self, order: std::sync::atomic::Ordering) -> u32 {
        self.value.load(order)
    }
}

/// Optional per-request `u16` high-water mark. See [`ObservedU64`].
#[derive(Debug, Default)]
pub struct ObservedU16 {
    #[cfg(feature = "semantic-observe")]
    value: std::sync::atomic::AtomicU16,
}

impl ObservedU16 {
    /// Raise the mark to at least `value`. A no-op without
    /// `semantic-observe`.
    #[inline(always)]
    pub fn fetch_max(&self, value: u16, order: std::sync::atomic::Ordering) {
        #[cfg(feature = "semantic-observe")]
        self.value.fetch_max(value, order);
        #[cfg(not(feature = "semantic-observe"))]
        let _ = (value, order);
    }

    /// Read the mark. Exists only where it is kept.
    #[cfg(feature = "semantic-observe")]
    #[must_use]
    pub fn load(&self, order: std::sync::atomic::Ordering) -> u16 {
        self.value.load(order)
    }
}

/// Optional per-request flag. See [`ObservedU64`].
#[derive(Debug, Default)]
pub struct ObservedFlag {
    #[cfg(feature = "semantic-observe")]
    value: std::sync::atomic::AtomicBool,
}

impl ObservedFlag {
    /// Raise the flag. A no-op without `semantic-observe`.
    #[inline(always)]
    pub fn raise(&self, order: std::sync::atomic::Ordering) {
        #[cfg(feature = "semantic-observe")]
        self.value.store(true, order);
        #[cfg(not(feature = "semantic-observe"))]
        let _ = order;
    }

    /// Read the flag. Exists only where it is kept.
    #[cfg(feature = "semantic-observe")]
    #[must_use]
    pub fn load(&self, order: std::sync::atomic::Ordering) -> bool {
        self.value.load(order)
    }
}

/// Optional phase clock for audit timings.
///
/// With `semantic-observe` it reads the clock at [`Stopwatch::start`];
/// without it it is a zero-size value that never reads the clock. Its
/// reader belongs inside a [`crate::observe!`] body, which a default build
/// never runs; reaching it there is a defect and panics rather than
/// reporting a fabricated duration.
#[derive(Debug, Clone, Copy)]
pub struct Stopwatch {
    #[cfg(feature = "semantic-observe")]
    started: Option<crate::instant::Instant>,
}

impl Stopwatch {
    /// Start the clock (no clock read without `semantic-observe`).
    #[inline(always)]
    #[must_use]
    pub fn start() -> Self {
        Self {
            #[cfg(feature = "semantic-observe")]
            started: Some(crate::instant::Instant::now()),
        }
    }

    /// Start the clock for a request that selected `mode` at its root
    /// entry: an uncaptured request never reads the clock, and its stopwatch
    /// is read only by a record it never publishes.
    #[inline(always)]
    #[must_use]
    pub fn start_for(mode: ObserveMode) -> Self {
        #[cfg(not(feature = "semantic-observe"))]
        let _ = mode;
        Self {
            #[cfg(feature = "semantic-observe")]
            started: mode.is_captured().then(crate::instant::Instant::now),
        }
    }

    /// Start the clock for execution compiled for `M`: the uncaptured
    /// execution never reads the clock and carries no mode test.
    #[inline(always)]
    #[must_use]
    pub fn start_in<M: ObserveExec>() -> Self {
        Self {
            #[cfg(feature = "semantic-observe")]
            started: M::CAPTURED.then(crate::instant::Instant::now),
        }
    }

    /// Milliseconds since [`Stopwatch::start`]; zero for an uncaptured
    /// request's stopwatch, which no published record reads.
    #[must_use]
    pub fn elapsed_ms(&self) -> f64 {
        #[cfg(feature = "semantic-observe")]
        {
            self.started
                .map_or(0.0, |started| started.elapsed().as_secs_f64() * 1000.0)
        }
        #[cfg(not(feature = "semantic-observe"))]
        {
            unreachable!("a phase clock is read only inside an observe! body")
        }
    }

    /// Nanoseconds since [`Stopwatch::start`], saturated to `u64`; zero for
    /// an uncaptured request's stopwatch.
    #[must_use]
    pub fn elapsed_nanos(&self) -> u64 {
        #[cfg(feature = "semantic-observe")]
        {
            self.started.map_or(0, |started| {
                started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64
            })
        }
        #[cfg(not(feature = "semantic-observe"))]
        {
            unreachable!("a phase clock is read only inside an observe! body")
        }
    }
}

/// An optional observation sampled where its source is in scope and read
/// later, inside a [`crate::observe!`] body.
///
/// With `semantic-observe`, [`Sample::take`] runs its closure and keeps the
/// value; without it the closure is never called and the sample is
/// zero-size. Reading a sample outside an `observe!` body in a default
/// build is a defect and panics rather than inventing a value.
pub struct Sample<T> {
    #[cfg(feature = "semantic-observe")]
    value: T,
    #[cfg(not(feature = "semantic-observe"))]
    _value: std::marker::PhantomData<fn() -> T>,
}

impl<T: Clone> Clone for Sample<T> {
    fn clone(&self) -> Self {
        Self {
            #[cfg(feature = "semantic-observe")]
            value: self.value.clone(),
            #[cfg(not(feature = "semantic-observe"))]
            _value: std::marker::PhantomData,
        }
    }
}

impl<T: Copy> Copy for Sample<T> {}

impl<T: std::fmt::Debug> std::fmt::Debug for Sample<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        #[cfg(feature = "semantic-observe")]
        {
            f.debug_tuple("Sample").field(&self.value).finish()
        }
        #[cfg(not(feature = "semantic-observe"))]
        {
            f.write_str("Sample(unavailable)")
        }
    }
}

impl<T> Sample<T> {
    /// Sample `collect()` when capture is compiled in.
    #[inline(always)]
    pub fn take(collect: impl FnOnce() -> T) -> Self {
        #[cfg(feature = "semantic-observe")]
        {
            Self { value: collect() }
        }
        #[cfg(not(feature = "semantic-observe"))]
        {
            let _ = collect;
            Self {
                _value: std::marker::PhantomData,
            }
        }
    }

    /// The sampled value.
    #[must_use]
    pub fn get(self) -> T {
        #[cfg(feature = "semantic-observe")]
        {
            self.value
        }
        #[cfg(not(feature = "semantic-observe"))]
        {
            unreachable!("an optional sample is read only inside an observe! body")
        }
    }
}
