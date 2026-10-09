//! Questions about a type answered through its parts from a work list.
//!
//! A question whose answer composes from the answers of a node's parts
//! (its union arms, intersection arms, member values, the node an alias
//! names) spends no native level per level of nesting here, and needs no
//! depth or hop ceiling: each walk visits a node once, so a finite graph
//! ends it, and a cycle ends it too. What the walk cannot answer stays
//! unanswered — it is never read as `false`.

use std::hash::Hash;

use rustc_hash::{FxHashMap, FxHashSet};

/// What one node says toward an existential question
/// ([`reaches`]).
pub enum Reach<K> {
    /// The node answers the question.
    Hit,
    /// The node answers nothing itself; these parts may.
    Parts(Vec<K>),
}

/// Whether any node reachable from `start` through `step`'s parts is a
/// [`Reach::Hit`]. Each key is stepped at most once.
pub fn reaches<K: Copy + Eq + Hash>(start: K, mut step: impl FnMut(K) -> Reach<K>) -> bool {
    let mut pending = vec![start];
    let mut seen: FxHashSet<K> = FxHashSet::default();
    while let Some(key) = pending.pop() {
        if !seen.insert(key) {
            continue;
        }
        match step(key) {
            Reach::Hit => return true,
            Reach::Parts(parts) => pending.extend(parts.into_iter().rev()),
        }
    }
    false
}

/// What one node says toward a three-valued question ([`classify`]).
pub(crate) enum Verdict<K> {
    /// The node's own answer: `None` when it cannot be decided.
    Leaf(Option<bool>),
    /// True when any part is true; false when every part is false;
    /// undecided otherwise.
    Any(Vec<K>),
    /// True when every part is true; false when any part is false;
    /// undecided otherwise.
    All(Vec<K>),
    /// The answer of the node this one stands for.
    As(K),
}

/// A node whose answer waits on its parts, with the nodes that stand for
/// it ([`Verdict::As`]).
struct Open<K> {
    key: K,
    stand_ins: Vec<K>,
    all: bool,
    parts: Vec<K>,
    next: usize,
    undecided: bool,
}

/// The answer to a three-valued question about `start`, composed through
/// `step`'s parts. A part that decides its node's answer (a true part of an
/// [`Verdict::Any`], a false part of an [`Verdict::All`]) ends the reading
/// of the node's other parts. A node reached again while its own answer is
/// still open — a cycle — is undecided.
///
/// `step` reads a node at most once once its answer is decided: a decided
/// answer rests only on decided parts, so a node shared by several others
/// is read once, however many paths reach it.
pub(crate) fn classify<K: Copy + Eq + Hash>(
    start: K,
    mut step: impl FnMut(K) -> Verdict<K>,
) -> Option<bool> {
    let mut open: Vec<Open<K>> = Vec::new();
    let mut on_path: FxHashSet<K> = FxHashSet::default();
    let mut known: FxHashMap<K, bool> = FxHashMap::default();
    let remember = |known: &mut FxHashMap<K, bool>, keys: &[K], verdict: Option<bool>| {
        if let Some(verdict) = verdict {
            known.extend(keys.iter().map(|key| (*key, verdict)));
        }
    };
    let mut next = Some(start);
    loop {
        // The answer of the node to read, when it answers without opening.
        let mut answer = None;
        if let Some(mut key) = next.take() {
            // The nodes read on the way, each standing for the next.
            let mut chain: Vec<K> = Vec::new();
            let mut chained: FxHashSet<K> = FxHashSet::default();
            answer = loop {
                if let Some(&verdict) = known.get(&key) {
                    remember(&mut known, &chain, Some(verdict));
                    break Some(Some(verdict));
                }
                if on_path.contains(&key) || !chained.insert(key) {
                    break Some(None);
                }
                chain.push(key);
                let (all, parts) = match step(key) {
                    Verdict::Leaf(verdict) => {
                        remember(&mut known, &chain, verdict);
                        break Some(verdict);
                    }
                    Verdict::As(other) => {
                        key = other;
                        continue;
                    }
                    Verdict::Any(parts) => (false, parts),
                    Verdict::All(parts) => (true, parts),
                };
                if parts.is_empty() {
                    // Any of nothing is false; all of nothing is true.
                    remember(&mut known, &chain, Some(all));
                    break Some(Some(all));
                }
                on_path.insert(key);
                chain.pop();
                open.push(Open {
                    key,
                    stand_ins: chain,
                    all,
                    parts,
                    next: 0,
                    undecided: false,
                });
                break None;
            };
        }
        let mut delivered = answer;
        loop {
            let Some(frame) = open.last_mut() else {
                return delivered.expect("the start's answer");
            };
            let decided = match delivered.take() {
                Some(Some(true)) if !frame.all => Some(true),
                Some(Some(false)) if frame.all => Some(false),
                Some(None) => {
                    frame.undecided = true;
                    None
                }
                _ => None,
            };
            if decided.is_none() {
                if let Some(part) = frame.parts.get(frame.next) {
                    frame.next += 1;
                    next = Some(*part);
                    break;
                }
            }
            let verdict = decided.or((!frame.undecided).then_some(frame.all));
            let closed = open.pop().expect("the open node");
            on_path.remove(&closed.key);
            remember(&mut known, &[closed.key], verdict);
            remember(&mut known, &closed.stand_ins, verdict);
            delivered = Some(verdict);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A chain of `Any` nodes `0 → 1 → … → n` ending at a leaf.
    fn any_chain(leaf: Option<bool>, length: u32) -> Option<bool> {
        classify(0u32, |key| {
            if key == length {
                Verdict::Leaf(leaf)
            } else {
                Verdict::Any(vec![key + 1])
            }
        })
    }

    #[test]
    fn a_chain_of_any_length_is_read_without_native_levels() {
        let answer = std::thread::Builder::new()
            .stack_size(1 << 20)
            .spawn(|| {
                (
                    any_chain(Some(true), 100_000),
                    any_chain(Some(false), 100_000),
                    any_chain(None, 100_000),
                )
            })
            .expect("spawn the classifying thread")
            .join()
            .expect("the classification returns");
        assert_eq!(answer, (Some(true), Some(false), None));
    }

    #[test]
    fn any_and_all_compose_with_undecided_parts() {
        // 0 = All[1, 2]; 1 = true; 2 = Any[3, 4]; 3 = undecided; 4 = true.
        let verdict = |key: u32| match key {
            0 => Verdict::All(vec![1, 2]),
            1 => Verdict::Leaf(Some(true)),
            2 => Verdict::Any(vec![3, 4]),
            3 => Verdict::Leaf(None),
            _ => Verdict::Leaf(Some(true)),
        };
        assert_eq!(classify(0u32, verdict), Some(true));
        // Without the deciding part, the undecided one leaves 2 undecided.
        let verdict = |key: u32| match key {
            0 => Verdict::All(vec![1, 2]),
            1 => Verdict::Leaf(Some(true)),
            2 => Verdict::Any(vec![3]),
            _ => Verdict::Leaf(None),
        };
        assert_eq!(classify(0u32, verdict), None);
        // A false part of an `All` decides it whatever the others are.
        let verdict = |key: u32| match key {
            0 => Verdict::All(vec![1, 2]),
            1 => Verdict::Leaf(None),
            _ => Verdict::Leaf(Some(false)),
        };
        assert_eq!(classify(0u32, verdict), Some(false));
    }

    #[test]
    fn a_cycle_is_undecided_and_a_redirect_cycle_too() {
        let verdict = |key: u32| match key {
            0 => Verdict::Any(vec![1]),
            _ => Verdict::As(0),
        };
        assert_eq!(classify(0u32, verdict), None);
        assert_eq!(classify(0u32, |key: u32| Verdict::As(1 - key)), None);
    }

    #[test]
    fn reaches_finds_a_hit_through_a_shared_part_and_ends_on_a_cycle() {
        // 0 → {1, 2}; 1 → {0}; 2 → {1, 3}; 3 is the hit.
        let step = |key: u32| match key {
            0 => Reach::Parts(vec![1, 2]),
            1 => Reach::Parts(vec![0]),
            2 => Reach::Parts(vec![1, 3]),
            _ => Reach::Hit,
        };
        assert!(reaches(0u32, step));
        let no_hit = |key: u32| Reach::Parts(vec![(key + 1) % 5]);
        assert!(!reaches(0u32, no_hit));
    }
}
