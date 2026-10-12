//! Cumulative process-tree CPU accounting. Reaped child usage is included
//! only to the extent it was not already observed on that child's identity.
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
    newly_missing: bool,
    reaped: bool,
    previous_children: (u64, u64),
    // Scratch state for the current missing-subtree reconciliation.
    covered: bool,
}

#[derive(Default)]
struct PendingEntry {
    children: usize,
    sampled: (u64, u64),
    newly_sampled: (u64, u64),
    uncertain: (u64, u64),
}

#[derive(Default)]
pub(crate) struct TreeCpu {
    entries: BTreeMap<Identity, Entry>,
    total: (u64, u64),
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
            entry.newly_missing = missing && !entry.missing;
            entry.missing = missing;
            entry.previous_children = (entry.reading.child_user, entry.reading.child_kernel);
        }
        for reading in readings {
            let identity = (reading.pid, reading.started);
            let parent = present.get(&reading.parent).copied();
            let entry = self.entries.entry(identity).or_insert_with(|| Entry {
                reading: reading.clone(),
                parent,
                missing: false,
                newly_missing: false,
                reaped: false,
                previous_children: (0, 0),
                covered: false,
            });
            // Reparented live descendants were orphaned, not reaped into
            // the old parent. Keep their usage independently.
            if reading.parent >= 0 && entry.reading.parent != reading.parent {
                entry.parent = parent;
            }
            entry.missing = false;
            entry.newly_missing = false;
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
        self.total = self.reconcile_missing();
    }

    pub fn totals(&self) -> (u64, u64) {
        self.total
    }

    fn reconcile_missing(&mut self) -> (u64, u64) {
        // Keep uncertain orphan usage separate from directly reaped children.
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
        let mut order = Vec::with_capacity(self.entries.len());
        while let Some(id) = ready.pop() {
            let folded = pending.remove(&id).expect("ready identity");
            let entry = &self.entries[&id];
            let r = &entry.reading;
            // Missing descendants of a missing parent may instead be orphans.
            // Require the newly missing subtree to fit in both counter deltas;
            // usage from earlier reaped children cannot cover a new orphan.
            let covered = r.child_user.saturating_sub(entry.previous_children.0)
                >= folded.newly_sampled.0.saturating_add(folded.uncertain.0)
                && r.child_kernel.saturating_sub(entry.previous_children.1)
                    >= folded.newly_sampled.1.saturating_add(folded.uncertain.1);
            let (uncertain_user, uncertain_kernel) =
                if covered { (0, 0) } else { folded.uncertain };
            let user = r.user.saturating_add(r.child_user.max(folded.sampled.0));
            let kernel = r
                .kernel
                .saturating_add(r.child_kernel.max(folded.sampled.1));
            let parent = entry.parent.filter(|_| entry.missing);
            let reaped = entry.reaped;
            let newly_missing = entry.newly_missing;
            self.entries.get_mut(&id).expect("folded identity").covered = covered;
            order.push(id);
            if let Some(parent) = parent {
                if let Some(folded) = pending.get_mut(&parent) {
                    let usage = if reaped {
                        &mut folded.sampled
                    } else {
                        &mut folded.uncertain
                    };
                    usage.0 = usage.0.saturating_add(user);
                    usage.1 = usage.1.saturating_add(kernel);
                    if reaped && newly_missing {
                        folded.newly_sampled.0 = folded.newly_sampled.0.saturating_add(user);
                        folded.newly_sampled.1 = folded.newly_sampled.1.saturating_add(kernel);
                    }
                    folded.uncertain.0 = folded.uncertain.0.saturating_add(uncertain_user);
                    folded.uncertain.1 = folded.uncertain.1.saturating_add(uncertain_kernel);
                    folded.children -= 1;
                    if folded.children == 0 {
                        ready.push(parent);
                    }
                    continue;
                }
            }
            total.0 = total.0.saturating_add(user).saturating_add(uncertain_user);
            total.1 = total
                .1
                .saturating_add(kernel)
                .saturating_add(uncertain_kernel);
        }
        // Resolve uncertain edges once, from ancestors to descendants. An
        // uncovered orphan stays independent: later unrelated child counters
        // must not retrospectively consume its retained CPU.
        for id in order.into_iter().rev() {
            let entry = &self.entries[&id];
            let inherited = entry
                .parent
                .filter(|_| entry.missing)
                .is_some_and(|parent| self.entries[&parent].covered);
            let entry = self.entries.get_mut(&id).expect("folded identity");
            if entry.missing && !entry.reaped && entry.parent.is_some() {
                if inherited {
                    entry.reaped = true;
                } else {
                    entry.parent = None;
                }
            }
            entry.covered |= inherited;
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
        assert_eq!(tree.totals(), (110, 55));
        // Reaping also captures the tail since the last child sample.
        tree.observe(vec![reading(1, 0, 20, 120)]);
        assert_eq!(tree.totals(), (140, 70));
        tree.observe(vec![reading(1, 0, 30, 120)]);
        assert_eq!(tree.totals(), (150, 75));
    }
    #[test]
    fn nested_reaping_between_samples_counts_each_lifetime_once() {
        for intermediate_sample in [false, true] {
            let mut tree = TreeCpu::default();
            tree.observe(vec![
                reading(1, 0, 10, 0),
                reading(2, 1, 20, 0),
                reading(3, 2, 100, 0),
            ]);
            assert_eq!(tree.totals(), (130, 65));
            if intermediate_sample {
                tree.observe(vec![reading(1, 0, 10, 0), reading(2, 1, 30, 110)]);
                assert_eq!(tree.totals(), (150, 75));
            }
            // The root's child counter includes the whole reaped subtree,
            // including unsampled CPU since the last child observations.
            tree.observe(vec![reading(1, 0, 20, 150)]);
            assert_eq!(tree.totals(), (170, 85));
            tree.observe(vec![reading(1, 0, 30, 150)]);
            assert_eq!(tree.totals(), (180, 90));
        }
    }

    #[test]
    fn vanished_orphan_retains_independently_reaped_parent_tail() {
        for previous_children in [0, 60, 600] {
            let mut tree = TreeCpu::default();
            tree.observe(vec![
                reading(1, 0, 10, previous_children),
                reading(2, 1, 20, 0),
                reading(3, 2, 100, 0),
            ]);
            assert_eq!(
                tree.totals(),
                (130 + previous_children, 65 + previous_children / 2)
            );
            // The parent spends ten more units, exits without waiting for its
            // child, and is reaped by the root. The orphan also exits before
            // another sample can observe its reparenting. Earlier reaped CPU
            // cannot serve as evidence that this subtree was reaped.
            tree.observe(vec![reading(1, 0, 20, previous_children + 30)]);
            assert_eq!(
                tree.totals(),
                (150 + previous_children, 75 + previous_children / 2)
            );
            tree.observe(vec![reading(1, 0, 30, previous_children + 30)]);
            assert_eq!(
                tree.totals(),
                (160 + previous_children, 80 + previous_children / 2)
            );
            // Later reaped children of the root cannot retrospectively absorb
            // an orphan that was already outside its reaped subtree.
            tree.observe(vec![reading(1, 0, 30, previous_children + 150)]);
            assert_eq!(
                tree.totals(),
                (280 + previous_children, 140 + previous_children / 2)
            );
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
        assert_eq!(tree.totals(), (290, 145));
        tree.observe(vec![reading(1, 0, 30, 140)]);
        assert_eq!(tree.totals(), (310, 155));
    }
    #[test]
    fn unsampled_reaped_children_and_orphaned_samples_are_retained() {
        let mut tree = TreeCpu::default();
        tree.observe(vec![reading(1, 0, 10, 60), reading(2, 1, 100, 0)]);
        tree.observe(vec![reading(1, 0, 20, 60), reading(2, 0, 140, 0)]);
        tree.observe(vec![reading(1, 0, 30, 60)]);
        assert_eq!(tree.totals(), (230, 115));
    }
    #[test]
    fn reused_pids_do_not_replace_previous_lifetimes() {
        let mut tree = TreeCpu::default();
        tree.observe(vec![reading(1, 0, 10, 0), reading(2, 1, 100, 0)]);
        tree.observe(vec![reading(1, 0, 20, 100)]);
        let mut reused = reading(2, 1, 40, 0);
        reused.started = 99;
        tree.observe(vec![reading(1, 0, 30, 100), reused]);
        assert_eq!(tree.totals(), (170, 85));
    }
}
