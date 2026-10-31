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

use crate::semantic_query::{property_spelling, AuthoredPropertyKey, PropertyKey};

#[cfg(any(test, feature = "test-support"))]
use crate::semantic_query::note_key_scan_comparison;

/// The identity `key` aggregates under: a known key's canonical spelling, or
/// the computed key itself.
fn canonical_identity(key: &AuthoredPropertyKey) -> AggregationKey {
    match key.cloned_known() {
        Some(known) => AggregationKey::known(known),
        None => AggregationKey(key.clone()),
    }
}

/// Canonical identity with work counted at hashing and equality, so a
/// scan behind the aggregation API pays for every comparison it makes.
#[derive(Eq)]
struct AggregationKey(AuthoredPropertyKey);

impl AggregationKey {
    fn known(key: PropertyKey) -> Self {
        Self(AuthoredPropertyKey::from_known(property_spelling(key)))
    }
}

impl PartialEq for AggregationKey {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        #[cfg(any(test, feature = "test-support"))]
        note_key_scan_comparison();
        self.0 == other.0
    }
}

impl std::hash::Hash for AggregationKey {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        #[cfg(any(test, feature = "test-support"))]
        note_key_scan_comparison();
        std::hash::Hash::hash(&self.0, state);
    }
}

/// The hash state of a key index. Member names are authored strings, so a
/// shipped index draws its own collision-resistant seed.
#[cfg(not(any(test, feature = "test-support")))]
type KeyHashState = std::collections::hash_map::RandomState;

/// The hash state of a key index in a build that counts key work: the keys a
/// probe compares follow from the seed, and each comparison counts, so a
/// fixed seed is what makes one key sequence cost one count on every run.
#[cfg(any(test, feature = "test-support"))]
type KeyHashState = std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>;

/// Accumulators of the properties a merge meets, in first-occurrence order.
pub struct OrderedMemberAggregation<A> {
    slots: HashMap<AggregationKey, usize, KeyHashState>,
    entries: Vec<(AuthoredPropertyKey, A)>,
}

impl<A> Default for OrderedMemberAggregation<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: std::fmt::Debug> std::fmt::Debug for OrderedMemberAggregation<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.entries.iter().map(|(_, accum)| accum))
            .finish()
    }
}

impl<A> OrderedMemberAggregation<A> {
    pub fn new() -> Self {
        Self {
            slots: HashMap::default(),
            entries: Vec::new(),
        }
    }

    /// An aggregation sized for `members` members, so the index never
    /// rehashes the keys it already holds.
    pub fn with_capacity(members: usize) -> Self {
        Self {
            slots: HashMap::with_capacity_and_hasher(members, KeyHashState::default()),
            entries: Vec::with_capacity(members),
        }
    }

    /// The accumulator of the property `key` addresses, created by `init` at
    /// the property's first occurrence, which also fixes its authored key and
    /// its output position.
    pub fn entry(&mut self, key: &AuthoredPropertyKey, init: impl FnOnce() -> A) -> &mut A {
        let next = self.entries.len();
        let slot = *self.slots.entry(canonical_identity(key)).or_insert(next);
        if slot == next {
            self.entries.push((key.clone(), init()));
        }
        &mut self.entries[slot].1
    }

    /// The output position of the property `key` addresses.
    pub fn position(&self, key: &AuthoredPropertyKey) -> Option<usize> {
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
    spellings: HashSet<AggregationKey, KeyHashState>,
}

impl PropertyKeySet {
    pub fn new(keys: &[PropertyKey]) -> Self {
        Self {
            spellings: keys.iter().cloned().map(AggregationKey::known).collect(),
        }
    }

    /// Whether `key` addresses a property of the set. A computed key
    /// addresses none.
    pub fn contains(&self, key: &AuthoredPropertyKey) -> bool {
        key.cloned_known()
            .is_some_and(|known| self.spellings.contains(&AggregationKey::known(known)))
    }
}

#[cfg(test)]
#[path = "aggregation_tests.rs"]
mod tests;
