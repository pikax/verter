//! Shared optional cache-occupancy measurement. The owner selects the
//! observation gate without enabling production capture in its dependencies.
//! Required retention decisions use current collection occupancy instead.

/// A cloneable count shared by cache stores. The reader exists only under
/// this owner's observation gate; production defaults retain no storage.
#[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
pub type OccupancyCounter =
    verter_audit::observe::Counter<std::sync::Arc<std::sync::atomic::AtomicU64>>;

/// Stripped cache measurement, with no reader or shared allocation.
#[cfg(not(any(test, feature = "test-support", feature = "semantic-observe")))]
pub type OccupancyCounter = verter_audit::observe::Counter<()>;
