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
    reaped: bool,
}

#[derive(Default)]
pub(crate) struct TreeCpu {
    entries: BTreeMap<Identity, Entry>,
}

impl TreeCpu {
    pub fn observe(&mut self, readings: Vec<CpuReading>) {
        let present: BTreeMap<_, _> = readings
            .iter()
            .map(|r| (r.pid, (r.pid, r.started)))
            .collect();
        for entry in self.entries.values_mut() {
            if present.get(&entry.reading.pid) != Some(&(entry.reading.pid, entry.reading.started))
                && entry.parent.is_some()
            {
                // A missing intermediate parent still carries this subtree
                // into its own parent's cumulative reaped-child counters.
                entry.reaped = true;
            }
        }
        for reading in readings {
            let identity = (reading.pid, reading.started);
            let parent = present.get(&reading.parent).copied();
            let entry = self.entries.entry(identity).or_insert_with(|| Entry {
                reading: reading.clone(),
                parent,
                reaped: false,
            });
            // Reparented live descendants were orphaned, not reaped into
            // the old parent. Keep their usage independently.
            if reading.parent >= 0 && entry.reading.parent != reading.parent {
                entry.parent = parent;
            }
            entry.reaped = false;
            entry.reading = reading;
        }
    }

    pub fn totals(&self) -> (u64, u64) {
        // Pending child count and reconciled child usage for each identity.
        // Fold leaves upward without recursion or repeated descendant scans.
        let mut pending: BTreeMap<_, _> = self
            .entries
            .keys()
            .map(|id| (*id, (0usize, 0u64, 0u64)))
            .collect();
        for entry in self.entries.values() {
            if let Some(parent) = entry.parent.filter(|_| entry.reaped) {
                if let Some((count, _, _)) = pending.get_mut(&parent) {
                    *count += 1;
                }
            }
        }
        let mut ready: Vec<_> = pending
            .iter()
            .filter_map(|(id, (count, _, _))| (*count == 0).then_some(*id))
            .collect();
        let mut total = (0u64, 0u64);
        while let Some(id) = ready.pop() {
            let (_, seen_user, seen_kernel) = pending.remove(&id).expect("ready identity");
            let entry = &self.entries[&id];
            let r = &entry.reading;
            let user = r.user.saturating_add(r.child_user.max(seen_user));
            let kernel = r.kernel.saturating_add(r.child_kernel.max(seen_kernel));
            if let Some(parent) = entry.parent.filter(|_| entry.reaped) {
                if let Some((count, child_user, child_kernel)) = pending.get_mut(&parent) {
                    *child_user = child_user.saturating_add(user);
                    *child_kernel = child_kernel.saturating_add(kernel);
                    *count -= 1;
                    if *count == 0 {
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
