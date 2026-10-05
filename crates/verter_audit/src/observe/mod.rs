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
/// Instrumented builds currently capture unconditionally. Root/job selection
/// and propagation across resumptions belong to the execution owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObserveMode {
    /// Execute without optional observations.
    Uncaptured,
    /// Execute with optional observations.
    Captured,
}

impl ObserveMode {
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
