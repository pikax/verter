//! Sets and maps of machine words the process mints itself.
//!
//! Measurements, owner bookkeeping and graph walks record which allocations
//! or nodes they have already met — keyed by allocation address or node
//! ordinal — and do so on hot paths. A general hash set spends a debug
//! build several calls per probe (its hasher, its group loads, their
//! precondition checks) where an open-addressed table of plain words needs
//! one multiply and a few compares. The keys are addresses and ordinals the
//! process allocates, never input-controlled, so a fixed multiplier cannot
//! be steered into collisions.

/// A set of `u64` words: open addressing over a power-of-two table with
/// linear probing, kept at most half full. Keys are full width on every
/// target — a node ordinal or token above `u32::MAX` is a distinct key on
/// a 32-bit target too — and an address widens into one losslessly.
#[derive(Default)]
pub struct WordSet {
    /// The table; `0` marks an empty slot. Empty until the first non-zero
    /// insert.
    slots: Box<[u64]>,
    /// Non-zero keys held.
    len: usize,
    /// Whether `0`, which marks an empty slot, is held.
    zero: bool,
    /// `64 - log2(slots.len())`: the shift that maps a key's mixed bits
    /// onto a slot.
    shift: u32,
}

/// Fibonacci-hashing multiplier: spreads aligned addresses, whose low bits
/// are all zero, across the high bits a slot index is read from.
const MIX: u64 = 0x9E37_79B9_7F4A_7C15;

/// Slots of the first table.
const FIRST_CAPACITY: usize = 32;

impl WordSet {
    /// An empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record `key`; `true` the first time it is offered, `false` after.
    #[inline]
    pub fn insert(&mut self, key: u64) -> bool {
        if key == 0 {
            let first = !self.zero;
            self.zero = true;
            return first;
        }
        if (self.len + 1) * 2 > self.slots.len() {
            self.grow();
        }
        let mask = self.slots.len() - 1;
        let mut index = self.slot_of(key);
        loop {
            let held = self.slots[index];
            if held == 0 {
                self.slots[index] = key;
                self.len += 1;
                return true;
            }
            if held == key {
                return false;
            }
            index = (index + 1) & mask;
        }
    }

    /// Whether `key` is held.
    #[must_use]
    pub fn contains(&self, key: u64) -> bool {
        if key == 0 {
            return self.zero;
        }
        if self.slots.is_empty() {
            return false;
        }
        let mask = self.slots.len() - 1;
        let mut index = self.slot_of(key);
        loop {
            let held = self.slots[index];
            if held == 0 {
                return false;
            }
            if held == key {
                return true;
            }
            index = (index + 1) & mask;
        }
    }

    /// Keys held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len + usize::from(self.zero)
    }

    /// Whether no key is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The held words, in no particular order.
    pub fn iter(&self) -> impl Iterator<Item = u64> + '_ {
        self.zero
            .then_some(0)
            .into_iter()
            .chain(self.slots.iter().copied().filter(|&key| key != 0))
    }

    #[inline]
    fn slot_of(&self, key: u64) -> usize {
        (key.wrapping_mul(MIX) >> self.shift) as usize
    }

    /// Double the table (or allocate the first) and re-place every key.
    fn grow(&mut self) {
        let capacity = if self.slots.is_empty() {
            FIRST_CAPACITY
        } else {
            self.slots.len() * 2
        };
        let old = std::mem::replace(&mut self.slots, vec![0; capacity].into_boxed_slice());
        self.shift = 64 - capacity.trailing_zeros();
        let mask = capacity - 1;
        let mut position = 0;
        while position < old.len() {
            let key = old[position];
            position += 1;
            if key == 0 {
                continue;
            }
            let mut index = self.slot_of(key);
            while self.slots[index] != 0 {
                index = (index + 1) & mask;
            }
            self.slots[index] = key;
        }
    }
}

impl std::fmt::Debug for WordSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WordSet").field("len", &self.len()).finish()
    }
}

impl FromIterator<u64> for WordSet {
    fn from_iter<I: IntoIterator<Item = u64>>(keys: I) -> Self {
        let mut set = Self::new();
        for key in keys {
            set.insert(key);
        }
        set
    }
}

/// A map from `u64` words to small `Copy` values: open addressing over a
/// power-of-two table with linear probing, kept at most half full. Every
/// word is a valid key.
pub struct WordMap<V: Copy> {
    /// The table: `None` marks an empty slot. Empty until the first insert.
    slots: Box<[Option<(u64, V)>]>,
    /// Keys held.
    len: usize,
    /// `64 - log2(slots.len())`.
    shift: u32,
}

impl<V: Copy> Default for WordMap<V> {
    fn default() -> Self {
        Self {
            slots: Box::default(),
            len: 0,
            shift: 0,
        }
    }
}

impl<V: Copy> WordMap<V> {
    /// An empty map.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The value held for `key`.
    #[must_use]
    pub fn get(&self, key: u64) -> Option<V> {
        if self.slots.is_empty() {
            return None;
        }
        let mask = self.slots.len() - 1;
        let mut index = self.slot_of(key);
        loop {
            match self.slots[index] {
                None => return None,
                Some((held, value)) if held == key => return Some(value),
                Some(_) => index = (index + 1) & mask,
            }
        }
    }

    /// Hold `value` for `key`, returning the value it replaces.
    pub fn insert(&mut self, key: u64, value: V) -> Option<V> {
        if (self.len + 1) * 2 > self.slots.len() {
            self.resize((self.slots.len() * 2).max(FIRST_CAPACITY));
        }
        let mask = self.slots.len() - 1;
        let mut index = self.slot_of(key);
        loop {
            match self.slots[index] {
                None => {
                    self.slots[index] = Some((key, value));
                    self.len += 1;
                    return None;
                }
                Some((held, previous)) if held == key => {
                    self.slots[index] = Some((key, value));
                    return Some(previous);
                }
                Some(_) => index = (index + 1) & mask,
            }
        }
    }

    /// Keep only the entries `keep` accepts.
    pub fn retain(&mut self, mut keep: impl FnMut(u64, V) -> bool) {
        let old = std::mem::take(&mut self.slots);
        self.len = 0;
        let mut kept = Vec::new();
        for (key, value) in old.iter().flatten().copied() {
            if keep(key, value) {
                kept.push((key, value));
            }
        }
        self.resize(kept.len() * 2);
        for (key, value) in kept {
            self.insert(key, value);
        }
    }

    /// Keys held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether no key is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn slot_of(&self, key: u64) -> usize {
        (key.wrapping_mul(MIX) >> self.shift) as usize
    }

    /// Re-place every entry in a table of at least `capacity` slots (an
    /// empty table when nothing is held and nothing is asked for).
    fn resize(&mut self, capacity: usize) {
        let capacity = if capacity == 0 && self.len == 0 {
            0
        } else {
            capacity.max(FIRST_CAPACITY).next_power_of_two()
        };
        let old = std::mem::replace(&mut self.slots, vec![None; capacity].into_boxed_slice());
        self.len = 0;
        if capacity == 0 {
            return;
        }
        self.shift = 64 - capacity.trailing_zeros();
        for (key, value) in old.iter().flatten().copied() {
            self.insert(key, value);
        }
    }
}

impl<V: Copy> std::fmt::Debug for WordMap<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WordMap").field("len", &self.len).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::WordSet;

    /// Every key is new exactly once, across zero, collisions and every
    /// growth of the table, and membership agrees with a reference set.
    #[test]
    fn each_word_is_new_once_across_growth() {
        let mut set = WordSet::new();
        let mut reference = std::collections::HashSet::new();
        // Aligned addresses (low bits zero), a run sharing high bits, and 0.
        let keys: Vec<u64> = (0..5000u64)
            .map(|i| (i * 7919 % 4099) * 64)
            .chain((0..64).map(|i| (1u64 << 40) + i * 8))
            .collect();
        for &key in &keys {
            assert_eq!(
                set.insert(key),
                reference.insert(key),
                "first offer of {key}"
            );
            assert!(set.contains(key));
        }
        for &key in &keys {
            assert!(!set.insert(key), "repeat offer of {key}");
        }
        assert_eq!(set.len(), reference.len());
        let mut held: Vec<u64> = set.iter().collect();
        held.sort_unstable();
        let mut expected: Vec<u64> = reference.into_iter().collect();
        expected.sort_unstable();
        assert_eq!(held, expected);
        assert!(!set.contains(3));
        assert!(!WordSet::new().contains(0));
        assert!(!WordSet::new().contains(64));
    }
}

#[cfg(test)]
mod full_width_tests {
    use super::WordSet;

    /// Keys that agree in their low 32 bits are distinct on every target,
    /// 32-bit ones included: a key is never narrowed to the address width.
    #[test]
    fn keys_sharing_low_bits_stay_distinct() {
        let mut set = WordSet::new();
        let keys = [5u64, (1 << 32) | 5, (1 << 63) | 5, u64::MAX, 0xFFFF_FFFF];
        for key in keys {
            assert!(set.insert(key), "first offer of {key:#x}");
        }
        for key in keys {
            assert!(set.contains(key) && !set.insert(key), "{key:#x}");
        }
        assert!(!set.contains(1 << 32));
        assert_eq!(set.len(), keys.len());
    }
}

#[cfg(test)]
mod word_map_tests {
    use super::WordMap;

    /// The map agrees with a reference map across zero keys, replacement,
    /// every growth of the table and a retain that drops most entries.
    #[test]
    fn entries_agree_with_a_reference_across_growth_and_retain() {
        let mut map = WordMap::<u32>::new();
        let mut reference = std::collections::HashMap::new();
        assert_eq!(map.get(0), None);
        let keys: Vec<u64> = (0..3000u64)
            .map(|i| (i * 7919 % 4099) << 32 | (i % 7))
            .chain([0, u64::MAX])
            .collect();
        for (value, &key) in keys.iter().enumerate() {
            let value = value as u32;
            assert_eq!(map.insert(key, value), reference.insert(key, value));
            assert_eq!(map.get(key), Some(value));
        }
        assert_eq!(map.len(), reference.len());
        map.retain(|key, _| key % 3 == 0);
        reference.retain(|key, _| key % 3 == 0);
        assert_eq!(map.len(), reference.len());
        for &key in &keys {
            assert_eq!(map.get(key), reference.get(&key).copied(), "{key}");
        }
        map.retain(|_, _| false);
        assert!(map.is_empty());
        assert_eq!(map.get(0), None);
        assert_eq!(map.insert(5, 1), None);
        assert_eq!(map.get(5), Some(1));
    }
}
