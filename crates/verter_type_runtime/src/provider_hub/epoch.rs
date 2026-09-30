//! The serving-epoch mint shared by every lifecycle cell in the hub.
//!
//! A [`ProviderEpoch`] names ONE serving incarnation of a provider instance: the
//! engine a hub installed after an establishment or a recovery, or the transport
//! a [`super::LazyTransport`] committed. Epochs are minted only here, only on a
//! successful install, and strictly increase per mint — so work stamped with an
//! older epoch is recognisably retired and can never settle against, mint a
//! receipt for, or warm the state of the incarnation that replaced it.

use std::sync::atomic::{AtomicU64, Ordering};

use verter_identity::identity::ProviderEpoch;

/// A per-instance monotonic [`ProviderEpoch`] source. The first minted epoch is
/// `1`; nothing is minted until an incarnation actually goes live.
#[derive(Debug)]
pub(crate) struct EpochMint {
    next: AtomicU64,
}

impl EpochMint {
    pub(crate) const fn new() -> Self {
        Self {
            next: AtomicU64::new(1),
        }
    }

    /// Whether any incarnation has gone live.
    pub(crate) fn minted_any(&self) -> bool {
        self.next.load(Ordering::Relaxed) > 1
    }

    /// Mint the epoch of an incarnation that is going live NOW.
    pub(crate) fn mint(&self) -> ProviderEpoch {
        ProviderEpoch(self.next.fetch_add(1, Ordering::Relaxed))
    }
}
