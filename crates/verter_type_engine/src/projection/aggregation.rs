//! Member aggregation keyed by canonical JS property identity.
//!
//! A surface merge (union arms, intersection arms, the macro member union, a
//! `Pick` selection) folds the members that address ONE property into one
//! entry. Two known keys address the same property exactly when their
//! [`property_spelling`]s are equal — `{ 1: x }` and `{ "1": y }` are one
//! property — while distinct unique symbols, and computed keys other than
//! themselves, stay distinct. Aggregating through an index over that
//! identity costs one probe per member; the output keeps its own
//! first-occurrence order (and each property's first authored key), so the
//! index's iteration order is never read.

use std::collections::{HashMap, HashSet};

use crate::semantic_query::{
    note_key_scan_comparison, property_spelling, AuthoredPropertyKey, PropertyKey,
};

/// The identity `key` aggregates under: a known key's canonical spelling, or
/// the computed key itself.
fn canonical_identity(key: &AuthoredPropertyKey) -> AuthoredPropertyKey {
    match key.cloned_known() {
        Some(known) => AuthoredPropertyKey::from_known(property_spelling(known)),
        None => key.clone(),
    }
}

/// Accumulators of the properties a merge meets, in first-occurrence order.
///
/// Member names are authored strings, so the index hashes with the default
/// collision-resistant hasher.
pub struct OrderedMemberAggregation<A> {
    slots: HashMap<AuthoredPropertyKey, usize>,
    entries: Vec<(AuthoredPropertyKey, A)>,
}

impl<A> Default for OrderedMemberAggregation<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A> OrderedMemberAggregation<A> {
    pub fn new() -> Self {
        Self {
            slots: HashMap::new(),
            entries: Vec::new(),
        }
    }

    /// The accumulator of the property `key` addresses, created by `init` at
    /// the property's first occurrence, which also fixes its authored key and
    /// its output position.
    pub fn entry(&mut self, key: &AuthoredPropertyKey, init: impl FnOnce() -> A) -> &mut A {
        note_key_scan_comparison();
        let next = self.entries.len();
        let slot = *self.slots.entry(canonical_identity(key)).or_insert(next);
        if slot == next {
            self.entries.push((key.clone(), init()));
        }
        &mut self.entries[slot].1
    }

    /// The output position of the property `key` addresses.
    pub fn position(&self, key: &AuthoredPropertyKey) -> Option<usize> {
        note_key_scan_comparison();
        self.slots.get(&canonical_identity(key)).copied()
    }

    /// The accumulator of the property `key` addresses.
    pub fn get(&self, key: &AuthoredPropertyKey) -> Option<&A> {
        self.position(key).map(|slot| &self.entries[slot].1)
    }

    /// The accumulator of the property `key` addresses, mutably.
    pub fn get_mut(&mut self, key: &AuthoredPropertyKey) -> Option<&mut A> {
        self.position(key).map(|slot| &mut self.entries[slot].1)
    }

    /// The same properties, in the same order, with each accumulator mapped
    /// through `f`; the index is kept.
    pub fn map<B>(
        self,
        mut f: impl FnMut(&AuthoredPropertyKey, A) -> B,
    ) -> OrderedMemberAggregation<B> {
        OrderedMemberAggregation {
            slots: self.slots,
            entries: self
                .entries
                .into_iter()
                .map(|(key, accum)| {
                    let mapped = f(&key, accum);
                    (key, mapped)
                })
                .collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Each property's first authored key and accumulator, in
    /// first-occurrence order.
    pub fn entries(&self) -> &[(AuthoredPropertyKey, A)] {
        &self.entries
    }

    /// [`Self::entries`], owned.
    pub fn into_entries(self) -> Vec<(AuthoredPropertyKey, A)> {
        self.entries
    }
}

/// A set of known keys answering membership under canonical property
/// identity, one probe per question.
pub struct PropertyKeySet {
    spellings: HashSet<PropertyKey>,
}

impl PropertyKeySet {
    pub fn new(keys: &[PropertyKey]) -> Self {
        Self {
            spellings: keys.iter().cloned().map(property_spelling).collect(),
        }
    }

    /// Whether `key` addresses a property of the set. A computed key
    /// addresses none.
    pub fn contains(&self, key: &AuthoredPropertyKey) -> bool {
        note_key_scan_comparison();
        key.cloned_known()
            .is_some_and(|known| self.spellings.contains(&property_spelling(known)))
    }
}

#[cfg(test)]
#[path = "aggregation_tests.rs"]
mod tests;
