//! Cumulative process-tree CPU accounting. Reaped child usage is included
//! only to the extent it was not already observed on that child's identity.
//! Ambiguous reaping histories make both CPU totals unavailable.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct CpuReading {
    pub pid: i32,
    pub parent: i32,
    pub started: u64,
    pub user: u64,
    pub kernel: u64,
    pub child_user: u64,
    pub child_kernel: u64,
}

type Identity = (i32, u64);
struct Entry {
    reading: CpuReading,
    parent: Option<Identity>,
    missing: bool,
    reaped: bool,
}

#[derive(Default)]
struct PendingEntry {
    children: usize,
    sampled: (u64, u64),
}

#[derive(Default)]
pub(crate) struct TreeCpu {
    entries: BTreeMap<Identity, Entry>,
    total: (u64, u64),
    ambiguous: bool,
}

impl TreeCpu {
    pub fn observe(&mut self, readings: Vec<CpuReading>) {
        let present: BTreeMap<_, _> = readings
            .iter()
            .map(|r| (r.pid, (r.pid, r.started)))
            .collect();
        for entry in self.entries.values_mut() {
            let missing = present.get(&entry.reading.pid)
                != Some(&(entry.reading.pid, entry.reading.started));
            entry.missing = missing;
        }
        for reading in readings {
            let identity = (reading.pid, reading.started);
            let parent = present.get(&reading.parent).copied();
            let entry = self.entries.entry(identity).or_insert_with(|| Entry {
                reading: reading.clone(),
                parent,
                missing: false,
                reaped: false,
            });
            // Reparented live descendants were orphaned, not reaped into
            // the old parent. Keep their usage independently.
            if reading.parent >= 0 && entry.reading.parent != reading.parent {
                entry.parent = parent;
            }
            entry.missing = false;
            entry.reaped = false;
            entry.reading = reading;
        }
        for entry in self.entries.values_mut() {
            if entry.missing
                && entry
                    .parent
                    .is_some_and(|id| present.get(&id.0) == Some(&id))
            {
                entry.reaped = true;
            }
        }
        // Only an observed surviving parent can attribute a disappearance to
        // its own child accounting. When both generations vanish, aggregate
        // counters cannot distinguish nested reaping from an orphan plus an
        // independent parent/sibling tail. Later samples cannot repair this
        // missing attribution, so never publish a numeric total again.
        self.ambiguous |= self
            .entries
            .values()
            .any(|entry| entry.missing && !entry.reaped && entry.parent.is_some());
        if !self.ambiguous {
            self.total = self.reconcile_missing();
        }
    }

    pub fn totals(&self) -> Option<(u64, u64)> {
        (!self.ambiguous).then_some(self.total)
    }

    fn reconcile_missing(&mut self) -> (u64, u64) {
        // Fold leaves upward without recursion or repeated descendant scans.
        let mut pending: BTreeMap<_, _> = self
            .entries
            .keys()
            .map(|id| (*id, PendingEntry::default()))
            .collect();
        for entry in self.entries.values() {
            if let Some(parent) = entry.parent.filter(|_| entry.missing) {
                if let Some(parent) = pending.get_mut(&parent) {
                    parent.children += 1;
                }
            }
        }
        let mut ready: Vec<_> = pending
            .iter()
            .filter_map(|(id, entry)| (entry.children == 0).then_some(*id))
            .collect();
        let mut total = (0u64, 0u64);
        while let Some(id) = ready.pop() {
            let folded = pending.remove(&id).expect("ready identity");
            let entry = &self.entries[&id];
            let r = &entry.reading;
            let user = r.user.saturating_add(r.child_user.max(folded.sampled.0));
            let kernel = r
                .kernel
                .saturating_add(r.child_kernel.max(folded.sampled.1));
            let parent = entry.parent.filter(|_| entry.missing);
            if let Some(parent) = parent {
                if let Some(folded) = pending.get_mut(&parent) {
                    folded.sampled.0 = folded.sampled.0.saturating_add(user);
                    folded.sampled.1 = folded.sampled.1.saturating_add(kernel);
                    folded.children -= 1;
                    if folded.children == 0 {
                        ready.push(parent);
                    }
                    continue;
                }
            }
            total.0 = total.0.saturating_add(user);
            total.1 = total.1.saturating_add(kernel);
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reading(pid: i32, parent: i32, user: u64, child_user: u64) -> CpuReading {
        CpuReading {
            pid,
            parent,
            started: pid as u64,
            user,
            kernel: user / 2,
            child_user,
            child_kernel: child_user / 2,
        }
    }
    #[test]
    fn reaped_children_replace_samples_without_double_counting() {
        let mut tree = TreeCpu::default();
        tree.observe(vec![reading(1, 0, 10, 0), reading(2, 1, 100, 0)]);
        assert_eq!(tree.totals(), Some((110, 55)));
        // Reaping also captures the tail since the last child sample.
        tree.observe(vec![reading(1, 0, 20, 120)]);
        assert_eq!(tree.totals(), Some((140, 70)));
        tree.observe(vec![reading(1, 0, 30, 120)]);
        assert_eq!(tree.totals(), Some((150, 75)));
    }
    #[test]
    fn nested_reaping_requires_observed_attribution() {
        for intermediate_sample in [false, true] {
            let mut tree = TreeCpu::default();
            tree.observe(vec![
                reading(1, 0, 10, 0),
                reading(2, 1, 20, 0),
                reading(3, 2, 100, 0),
            ]);
            assert_eq!(tree.totals(), Some((130, 65)));
            if intermediate_sample {
                tree.observe(vec![reading(1, 0, 10, 0), reading(2, 1, 30, 110)]);
                assert_eq!(tree.totals(), Some((150, 75)));
            }
            // With the intermediate observation the root's child counter
            // covers the already attributed subtree, including its unseen
            // tails. Without it, an orphan history is indistinguishable.
            tree.observe(vec![reading(1, 0, 20, 150)]);
            assert_eq!(tree.totals(), intermediate_sample.then_some((170, 85)));
            tree.observe(vec![reading(1, 0, 30, 150)]);
            assert_eq!(tree.totals(), intermediate_sample.then_some((180, 90)));
        }
    }

    #[test]
    fn vanished_descendant_makes_cpu_unavailable_permanently() {
        for previous_children in [0, 60, 600] {
            for child_tail in [30, 150] {
                let mut tree = TreeCpu::default();
                tree.observe(vec![
                    reading(1, 0, 10, previous_children),
                    reading(2, 1, 20, 0),
                    reading(3, 2, 100, 0),
                ]);
                assert_eq!(
                    tree.totals(),
                    Some((130 + previous_children, 65 + previous_children / 2))
                );
                // The parent and grandchild disappear together. A tail of 150
                // can be nested reaping, the parent's own CPU with an orphan,
                // or a smaller parent tail plus an unseen reaped sibling.
                // Neither counter magnitude nor earlier child CPU identifies
                // the reaped lifetimes. The user and kernel totals are unknown.
                tree.observe(vec![reading(1, 0, 20, previous_children + child_tail)]);
                assert_eq!(tree.totals(), None);
                tree.observe(vec![reading(1, 0, 30, previous_children + child_tail)]);
                assert_eq!(tree.totals(), None);
                // Later unrelated child CPU cannot resolve that history.
                tree.observe(vec![reading(
                    1,
                    0,
                    30,
                    previous_children + child_tail + 120,
                )]);
                assert_eq!(tree.totals(), None);
            }
        }
    }

    #[test]
    fn live_orphan_and_reused_parent_stay_outside_the_reaped_subtree() {
        let mut tree = TreeCpu::default();
        tree.observe(vec![
            reading(1, 0, 10, 60),
            reading(2, 1, 20, 0),
            reading(3, 2, 100, 0),
        ]);
        let mut reused = reading(2, 1, 40, 0);
        reused.started = 99;
        tree.observe(vec![reading(1, 0, 20, 90), reused, reading(3, 0, 140, 0)]);
        assert_eq!(tree.totals(), Some((290, 145)));
        tree.observe(vec![reading(1, 0, 30, 140)]);
        assert_eq!(tree.totals(), Some((310, 155)));
    }
    #[test]
    fn unsampled_reaped_children_and_orphaned_samples_are_retained() {
        let mut tree = TreeCpu::default();
        tree.observe(vec![reading(1, 0, 10, 60), reading(2, 1, 100, 0)]);
        tree.observe(vec![reading(1, 0, 20, 60), reading(2, 0, 140, 0)]);
        tree.observe(vec![reading(1, 0, 30, 60)]);
        assert_eq!(tree.totals(), Some((230, 115)));
    }
    #[test]
    fn reused_pids_do_not_replace_previous_lifetimes() {
        let mut tree = TreeCpu::default();
        tree.observe(vec![reading(1, 0, 10, 0), reading(2, 1, 100, 0)]);
        tree.observe(vec![reading(1, 0, 20, 100)]);
        let mut reused = reading(2, 1, 40, 0);
        reused.started = 99;
        tree.observe(vec![reading(1, 0, 30, 100), reused]);
        assert_eq!(tree.totals(), Some((170, 85)));
    }
}
