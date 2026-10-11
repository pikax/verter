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
                && entry.parent.is_some_and(|p| present.get(&p.0) == Some(&p))
            {
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
            entry.reading = reading;
        }
    }

    pub fn totals(&self) -> (u64, u64) {
        self.entries
            .iter()
            .fold((0, 0), |(user, kernel), (id, entry)| {
                let (seen_user, seen_kernel) = self
                    .entries
                    .values()
                    .filter(|child| child.reaped && child.parent == Some(*id))
                    .fold((0u64, 0u64), |(u, k), child| {
                        (
                            u.saturating_add(
                                child.reading.user.saturating_add(child.reading.child_user),
                            ),
                            k.saturating_add(
                                child
                                    .reading
                                    .kernel
                                    .saturating_add(child.reading.child_kernel),
                            ),
                        )
                    });
                let r = &entry.reading;
                (
                    user.saturating_add(r.user)
                        .saturating_add(r.child_user.saturating_sub(seen_user)),
                    kernel
                        .saturating_add(r.kernel)
                        .saturating_add(r.child_kernel.saturating_sub(seen_kernel)),
                )
            })
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
