//! Resolved module bindings, shared by namespace value demands.

use std::sync::Arc;

use crate::facts::fact_cache::FactVersionRef;
use crate::retention::RetentionCharge;
use crate::type_solver::ResolvedRootIdentity;

pub type ModuleExportBindings = imbl::GenericHashMap<
    Arc<str>,
    Option<ModuleExportBinding>,
    rustc_hash::FxBuildHasher,
    imbl::shared_ptr::DefaultSharedPtr,
>;

pub type ModuleExportBindingRef<'a> = (&'a Arc<str>, &'a Option<ModuleExportBinding>);

#[derive(Debug, Clone)]
pub struct ModuleExportTable {
    /// `None` is an ambiguous star binding. Retaining it makes ambiguity
    /// propagate through later barrels; absence alone would lose that fact.
    pub bindings: ModuleExportBindings,
    pub facts: Arc<[FactVersionRef]>,
    /// The charge follows the shared HAMT nodes, including nodes inherited
    /// from dependency tables after their cache entries have been evicted.
    pub retention: Option<Arc<ModuleExportRetention>>,
    /// Writes to this table's persistent binding structure, excluding the
    /// child structure shared unchanged. Used for footprint/work evidence.
    pub owned_binding_slots: usize,
}

#[derive(Debug)]
pub struct ModuleExportRetention {
    pub charge: RetentionCharge,
    pub dependencies: Vec<Arc<ModuleExportRetention>>,
}

impl Drop for ModuleExportRetention {
    fn drop(&mut self) {
        let mut pending = std::mem::take(&mut self.dependencies);
        while let Some(dependency) = pending.pop() {
            if let Ok(mut owned) = Arc::try_unwrap(dependency) {
                pending.append(&mut owned.dependencies);
            }
        }
    }
}

impl ModuleExportTable {
    /// Lexical export order in linear byte work, independent of hash-map
    /// insertion order. The returned count includes every radix visit.
    pub fn ordered_bindings(&self) -> (Vec<ModuleExportBindingRef<'_>>, usize) {
        let mut bindings: Vec<_> = self.bindings.iter().collect();
        let work = sort_bytes(&mut bindings, |(name, _)| name.as_bytes());
        (bindings, work)
    }
}

/// MSD radix order over UTF-8 bytes, with zero terminating a name. Small
/// terminal buckets use bounded insertion work; no whole-input comparison
/// sort runs. The explicit stack bounds native stack use for long names.
pub(crate) fn sort_bytes<T>(items: &mut [T], key: impl Fn(&T) -> &[u8]) -> usize {
    const SMALL_BUCKET: usize = 16;
    let mut pending = vec![(0, items.len(), 0)];
    let mut work = 0;
    while let Some((start, end, mut byte)) = pending.pop() {
        if end - start < 2 {
            continue;
        }
        // Consume a bucket's common prefix once, including for insertion
        // buckets whose comparisons would otherwise reread it per pair.
        let first_key = key(&items[start]);
        let mut prefix_end = first_key.len();
        for item in &items[start + 1..end] {
            if prefix_end == byte {
                break;
            }
            let other = key(item);
            work += 1;
            let limit = prefix_end.min(other.len());
            let mut at = byte;
            while at < limit {
                work += 1;
                if first_key[at] != other[at] {
                    break;
                }
                at += 1;
            }
            prefix_end = at;
        }
        byte = prefix_end;
        if end - start <= SMALL_BUCKET {
            for at in start + 1..end {
                let mut at = at;
                while at > start {
                    let left = &key(&items[at - 1])[byte..];
                    let right = &key(&items[at])[byte..];
                    let mut order = left.len().cmp(&right.len());
                    for (a, b) in left.iter().zip(right) {
                        work += 1;
                        if a != b {
                            order = a.cmp(b);
                            break;
                        }
                    }
                    work += 1;
                    if order != std::cmp::Ordering::Greater {
                        break;
                    }
                    items.swap(at - 1, at);
                    at -= 1;
                }
            }
            continue;
        }
        let symbol = |item: &T| key(item).get(byte).map_or(0, |b| usize::from(*b) + 1);
        let mut counts = [0usize; 257];
        let mut first = 256;
        let mut last = 0;
        for item in &items[start..end] {
            let bucket = symbol(item);
            counts[bucket] += 1;
            first = first.min(bucket);
            last = last.max(bucket);
            work += 1;
        }
        // A uniform bucket after the common prefix consists of equal names
        // that have all terminated; it needs no distribution.
        if first == last {
            continue;
        }
        let mut starts = [0usize; 258];
        starts[first] = start;
        for bucket in first..=last {
            starts[bucket + 1] = starts[bucket] + counts[bucket];
            work += 1;
        }
        let mut next = starts;
        for bucket in first..=last {
            work += 1;
            while next[bucket] < starts[bucket + 1] {
                let destination = symbol(&items[next[bucket]]);
                work += 1;
                if destination == bucket {
                    next[bucket] += 1;
                } else {
                    items.swap(next[bucket], next[destination]);
                    next[destination] += 1;
                }
            }
            if bucket > 0 && counts[bucket] > 1 {
                pending.push((starts[bucket], starts[bucket + 1], byte + 1));
            }
        }
    }
    work
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleExportBinding {
    Value(ResolvedRootIdentity),
    Namespace(Arc<str>),
}

#[cfg(test)]
mod tests {
    #[test]
    fn byte_order_work_tracks_names_with_a_long_shared_prefix() {
        let prefix = "shared/".repeat(32);
        for count in [16, 40] {
            let expected: Vec<_> = (0..count).map(|i| format!("{prefix}{i:02}")).collect();
            let mut names = expected.clone();
            names.reverse();
            let visits = super::sort_bytes(&mut names, |name| name.as_bytes());
            assert_eq!(names, expected);
            let bytes: usize = names.iter().map(String::len).sum();
            assert!(
                visits <= 4 * bytes,
                "{visits} visits for {bytes} name bytes"
            );
        }
    }

    #[test]
    fn byte_order_keeps_prefixes_terminators_and_unicode() {
        let mut names = [
            "中", "a\0b", "aa", "é", "b", "a", "", "a\0", "a", "e", "zz", "000", "00", "0", "AAA",
            "Aa", "z", "😀", "😀a", "中a", "aé",
        ];
        let visits = super::sort_bytes(&mut names, |name| name.as_bytes());
        assert_eq!(
            names,
            [
                "", "0", "00", "000", "AAA", "Aa", "a", "a", "a\0", "a\0b", "aa", "aé", "b", "e",
                "z", "zz", "é", "中", "中a", "😀", "😀a"
            ]
        );
        assert!(visits > 0);
        for name in ["", "same"] {
            let mut equal = [name; 32];
            super::sort_bytes(&mut equal, |name| name.as_bytes());
            assert_eq!(equal, [name; 32], "equal names terminate together");
        }
    }
}
