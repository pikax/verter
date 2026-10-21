//! Read-only observations of allocation-owned charges.
//!
//! A held semantic answer keeps allocations that carry their own charge
//! ([`super::owned::Owned`] cells) and owner metadata charged through a
//! [`CallerHold`]. The account's totals cannot say which of those bytes a
//! given answer keeps: other holders, other hosts and unrelated work share
//! the one process account. A [`ChargeObservation`] reads the charges of the
//! exact allocations a holder reaches instead, one entry per allocation.
//!
//! # What an entry says
//!
//! * **Identity** — the allocation's address ([`AllocationId`]). Every clone
//!   of a handle names the same allocation, so two observations of copies of
//!   one answer name the same ids. An id is meaningful only while the
//!   observed holder keeps the allocation: the observation holds no handle,
//!   so once the allocation is freed its address may be reused.
//! * **Charge** — the class and bytes the allocation's own charge holds at
//!   the instant it was read; no class while no owner has charged it.
//! * **Measured bytes** — the figure the allocation's charge must equal,
//!   taken by the same measurement a first charge takes, from the value
//!   itself. It does not depend on who holds the charge, on the transfer
//!   being checked, on a repeat of the computation or on how full the
//!   account is, so a charge that was dropped, doubled or partly moved
//!   disagrees with it whatever any other observation says.
//! * **Shared block** — for an allocation weak views (an interner) may keep
//!   allocated past its value, the pinned charge on its block, which lasts
//!   until both the value and every weak view are gone.
//!
//! # Closure and deduplication
//!
//! An observation covers the handles it is given and, transitively, every
//! owned child they answer for: the children a charged cell recorded when it
//! was first charged, or for an uncharged cell those a first charge would
//! record. Each allocation appears ONCE however many parents or handle sets
//! reach it — the first reach decides whether it is [`Reach::Direct`] (named
//! by a handle the observer was given) or [`Reach::Nested`] (reached only
//! through another owned allocation). That is the same rule the charge
//! lifecycle follows: a nested allocation is charged by its own cell, once.
//!
//! # What reading costs
//!
//! Nothing that outlives the read. An observation keeps addresses and
//! numbers, never a handle, so it extends no allocation's lifetime; it takes
//! no reservation, adopts no pin and releases nothing, so the account's
//! classes and totals are the same before and after it.

use std::sync::Arc;

use rustc_hash::FxHashSet;

use super::owned::{read_node, Measured, Node, Owned, OwnedHandles};
use super::shared::CallerHold;
use super::{ChargeClass, RetentionCharge};

/// One allocation's address: an identity that is never dereferenced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AllocationId(usize);

/// How an observation reached an allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reach {
    /// Named by a handle the observer was given.
    Direct,
    /// Reached only as an owned child of another observed allocation.
    Nested,
}

/// The pinned charge on an allocation's block shared with its weak views.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SharedBlockCharge {
    /// Bytes the share holds.
    pub charged_bytes: usize,
    /// Bytes the share must hold: the cell's block and the share's own.
    pub measured_bytes: usize,
}

/// One allocation's own charge, read at one instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AllocationCharge {
    /// Which allocation.
    pub allocation: AllocationId,
    /// How the observation reached it.
    pub reach: Reach,
    /// Its charge's class; `None` while no owner has charged it.
    pub class: Option<ChargeClass>,
    /// Bytes its charge holds; zero while uncharged.
    pub charged_bytes: usize,
    /// Bytes its charge must hold, measured from the allocation itself.
    pub measured_bytes: usize,
    /// The pinned share on its block, for an allocation weak views keep.
    pub shared_block: Option<SharedBlockCharge>,
}

impl AllocationCharge {
    /// Whether the allocation is charged, and charged exactly what it
    /// measures — its block share included.
    #[must_use]
    pub fn is_exact(&self) -> bool {
        self.class.is_some()
            && self.charged_bytes == self.measured_bytes
            && self
                .shared_block
                .is_none_or(|block| block.charged_bytes == block.measured_bytes)
    }
}

/// An immutable set of allocation charges, each allocation once, ordered by
/// identity. See the module docs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChargeObservation {
    entries: Box<[AllocationCharge]>,
}

impl ChargeObservation {
    /// The entries, ordered by allocation identity.
    #[must_use]
    pub fn entries(&self) -> &[AllocationCharge] {
        &self.entries
    }

    /// The entry for `allocation`, if observed.
    #[must_use]
    pub fn get(&self, allocation: AllocationId) -> Option<&AllocationCharge> {
        self.entries
            .binary_search_by_key(&allocation, |entry| entry.allocation)
            .ok()
            .map(|at| &self.entries[at])
    }

    /// The observed allocations, ordered.
    pub fn allocations(&self) -> impl Iterator<Item = AllocationId> + '_ {
        self.entries.iter().map(|entry| entry.allocation)
    }

    /// How many allocations were observed.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no allocation was observed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Bytes the observed allocations' own charges hold, block shares
    /// excluded.
    #[must_use]
    pub fn charged_bytes(&self) -> usize {
        self.entries.iter().map(|entry| entry.charged_bytes).sum()
    }

    /// Bytes the observed allocations' own charges of `class` hold.
    #[must_use]
    pub fn charged_bytes_in(&self, class: ChargeClass) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.class == Some(class))
            .map(|entry| entry.charged_bytes)
            .sum()
    }

    /// Bytes the observed allocations measure, block shares excluded: what
    /// their charges must hold together.
    #[must_use]
    pub fn measured_bytes(&self) -> usize {
        self.entries.iter().map(|entry| entry.measured_bytes).sum()
    }

    /// Bytes the observed block shares hold.
    #[must_use]
    pub fn shared_block_bytes(&self) -> usize {
        self.entries
            .iter()
            .filter_map(|entry| entry.shared_block)
            .map(|block| block.charged_bytes)
            .sum()
    }

    /// Whether every observed allocation is charged exactly what it
    /// measures.
    #[must_use]
    pub fn is_exact(&self) -> bool {
        self.entries.iter().all(AllocationCharge::is_exact)
    }
}

/// Builds one [`ChargeObservation`] over several holders, each allocation
/// once. See the module docs.
#[derive(Default)]
pub struct ChargeObserver {
    seen: FxHashSet<AllocationId>,
    entries: Vec<AllocationCharge>,
}

impl ChargeObserver {
    /// An observer that has seen nothing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Skip the allocations `observed` already names, so observations of
    /// several sections of one holder stay disjoint: an allocation two
    /// sections reach belongs to the one observed first.
    pub fn exclude(&mut self, observed: &ChargeObservation) {
        self.seen.extend(observed.allocations());
    }

    /// Observe one owned allocation and the owned children it answers for.
    pub fn owned<T: Measured>(&mut self, handle: &Owned<T>) {
        self.node(&handle.node(), Reach::Direct);
    }

    /// Observe the allocations `handles` names and the owned children they
    /// answer for.
    pub fn owned_handles(&mut self, handles: &OwnedHandles) {
        for node in handles.nodes() {
            self.node(node, Reach::Direct);
        }
    }

    /// Observe the charge a caller hold shares, against `measured_bytes`:
    /// the bytes its holder's own measurement says the charge must hold.
    pub fn caller_hold(&mut self, hold: &CallerHold, measured_bytes: usize) {
        let allocation = AllocationId(hold.identity());
        if !self.seen.insert(allocation) {
            return;
        }
        self.entries.push(AllocationCharge {
            allocation,
            reach: Reach::Direct,
            class: Some(hold.class()),
            charged_bytes: hold.bytes(),
            measured_bytes,
            shared_block: None,
        });
    }

    /// Observe an allocation that keeps its own `charge` beside its value —
    /// `None` while no owner has charged it — against `measured_bytes`, the
    /// figure the allocation's own measurement says the charge must hold.
    /// `held` names the owned allocations the charge keeps cache holds on;
    /// each is observed as a nested child. Returns whether the allocation
    /// was new to this observer, so a caller walking a closure of such
    /// allocations visits each once.
    pub fn self_charged<T>(
        &mut self,
        allocation: &Arc<T>,
        reach: Reach,
        charge: Option<&RetentionCharge>,
        measured_bytes: usize,
        held: &OwnedHandles,
    ) -> bool {
        let allocation = AllocationId(Arc::as_ptr(allocation).cast::<()>() as usize);
        if !self.seen.insert(allocation) {
            return false;
        }
        self.entries.push(AllocationCharge {
            allocation,
            reach,
            class: charge.map(RetentionCharge::class),
            charged_bytes: charge.map_or(0, RetentionCharge::bytes),
            measured_bytes,
            shared_block: None,
        });
        for node in held.nodes() {
            self.node(node, Reach::Nested);
        }
        true
    }

    /// The observation, ordered by allocation identity.
    #[must_use]
    pub fn finish(mut self) -> ChargeObservation {
        self.entries.sort_unstable_by_key(|entry| entry.allocation);
        ChargeObservation {
            entries: self.entries.into_boxed_slice(),
        }
    }

    fn node(&mut self, node: &Arc<dyn Node>, reach: Reach) {
        let allocation = AllocationId(Arc::as_ptr(node).cast::<()>() as usize);
        if !self.seen.insert(allocation) {
            return;
        }
        let reading = read_node(&**node);
        self.entries.push(AllocationCharge {
            allocation,
            reach,
            class: reading.class,
            charged_bytes: reading.charged_bytes,
            measured_bytes: reading.measured_bytes,
            shared_block: reading.shared_block.map(|(charged_bytes, measured_bytes)| {
                SharedBlockCharge {
                    charged_bytes,
                    measured_bytes,
                }
            }),
        });
        for child in reading.children.iter() {
            self.node(child, Reach::Nested);
        }
    }
}

#[cfg(test)]
#[path = "witness_tests.rs"]
mod tests;
