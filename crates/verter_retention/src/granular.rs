//! A pinned charge that follows live backing storage as it grows and
//! shrinks.
//!
//! Some storage is held by the live graph itself rather than by a cache:
//! an arena's chunks and node payloads, a signature epoch's tables. It is a
//! liveness obligation — refusing it would refuse the computation — so it is
//! [`ChargeClass::Pinned`], charged unconditionally, consuming headroom that
//! discretionary caches then cannot take.
//!
//! Such storage changes one allocation at a time, on paths too hot to take
//! the account's lock for each. A [`GranularPin`] therefore settles its
//! charge in whole granules: the charge is the live total rounded UP to the
//! granule, raised the moment the total crosses it and lowered once the
//! total falls two granules below (or to nothing). The figure is never below the live
//! total, so the storage is never under-charged; it exceeds it by less than
//! two granules — the declared convention — and the account's lock is taken
//! once per granule of change, not once per allocation.

use std::sync::Arc;

use super::{ChargeClass, RetentionCharge, SemanticRetentionAccount};

/// A pinned charge settled in granules over a live byte total. See the
/// module docs.
#[derive(Debug)]
pub struct GranularPin {
    granule: usize,
    used: usize,
    charge: RetentionCharge,
}

impl GranularPin {
    /// A pin of nothing yet against `account`, settled in `granule`-byte
    /// steps.
    ///
    /// # Panics
    ///
    /// When `granule` is zero.
    #[must_use]
    pub fn new(account: &Arc<SemanticRetentionAccount>, granule: usize) -> Self {
        assert!(granule > 0, "a granule holds at least one byte");
        Self {
            granule,
            used: 0,
            charge: account.pin(0),
        }
    }

    /// `bytes` more of live storage.
    pub fn add(&mut self, bytes: usize) {
        self.used += bytes;
        if self.used > self.charge.bytes() {
            let target = self.used.div_ceil(self.granule) * self.granule;
            let account = self
                .charge
                .account
                .as_ref()
                .expect("a live pin has its account");
            let more = account.pin(target - self.charge.bytes());
            self.charge.absorb(more);
        }
    }

    /// `bytes` of live storage freed.
    ///
    /// # Panics
    ///
    /// When more is freed than was added: the caller's storage accounting
    /// is broken, and clamping would hide the miscount.
    pub fn sub(&mut self, bytes: usize) {
        self.used = self
            .used
            .checked_sub(bytes)
            .expect("freed no more storage than was added");
        let keep = self.used.div_ceil(self.granule) * self.granule;
        // Storage that is all freed keeps nothing charged.
        if self.charge.bytes() >= keep + 2 * self.granule
            || (self.used == 0 && self.charge.bytes() > 0)
        {
            let surplus = self.charge.bytes() - keep;
            drop(self.charge.split_off(surplus));
        }
    }

    /// The live total.
    #[must_use]
    pub fn used(&self) -> usize {
        self.used
    }

    /// The bytes currently charged: at least [`Self::used`], by less than
    /// two granules.
    #[must_use]
    pub fn charged(&self) -> usize {
        self.charge.bytes()
    }
}

impl RetentionCharge {
    /// Fold `other` — a charge of the same class on the same account — into
    /// this one: the bytes stay charged exactly once, now released by this
    /// charge.
    ///
    /// # Panics
    ///
    /// When the two charges belong to different accounts or classes: merging
    /// them would release bytes from an account that never charged them. Also
    /// when the class is [`ChargeClass::Active`]: in-flight work is never
    /// merged, so the merge moves no ACTIVE attribution.
    pub fn absorb(&mut self, mut other: RetentionCharge) {
        let same_account = match (self.account.as_ref(), other.account.as_ref()) {
            (Some(mine), Some(theirs)) => Arc::ptr_eq(mine, theirs),
            _ => false,
        };
        assert!(
            same_account && self.class == other.class,
            "absorb merges charges of one class on one account"
        );
        assert_ne!(
            self.class,
            ChargeClass::Active,
            "absorb merges settled charges, never in-flight ACTIVE work"
        );
        let merged = self.provenance.merged(other.provenance);
        self.bytes += other.bytes;
        self.provenance = merged;
        // The absorbed charge's bytes are this charge's now: it must not
        // release them.
        other.account = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RetentionLimits;

    #[test]
    fn the_charge_covers_the_live_total_within_two_granules_and_is_released_once() {
        let account = SemanticRetentionAccount::new(RetentionLimits::defaults());
        let mut pin = GranularPin::new(&account, 100);
        pin.add(1);
        assert_eq!(pin.charged(), 100);
        pin.add(250);
        assert_eq!((pin.used(), pin.charged()), (251, 300));
        assert_eq!(account.snapshot().pinned_bytes, 300);
        pin.sub(150);
        assert_eq!(
            pin.charged(),
            300,
            "a shrink inside two granules keeps the charge"
        );
        pin.sub(101);
        assert_eq!((pin.used(), pin.charged()), (0, 0));
        assert_eq!(account.snapshot().pinned_bytes, 0);
        pin.add(10);
        drop(pin);
        assert_eq!(account.snapshot().total_bytes(), 0);
    }

    #[test]
    fn a_storage_pin_is_charged_past_the_ceiling() {
        let account = SemanticRetentionAccount::new(RetentionLimits {
            aggregate_ceiling_bytes: 0,
            ..RetentionLimits::defaults()
        });
        let mut pin = GranularPin::new(&account, 64);
        pin.add(1_000);
        assert!(account.snapshot().pinned_bytes >= 1_000);
    }
}
