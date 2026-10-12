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
    /// `NOTE_EXIT` sample taken while this identity's parent was still present.
    /// A later snapshot where both are gone does not make that history ambiguous,
    /// and these counters are the floor for the lifetime.
    exit_evidenced: bool,
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
    upper: (u64, u64),
    ambiguous: bool,
}

impl TreeCpu {
    pub fn record_exit(
        &mut self,
        reading: CpuReading,
        current_start: impl FnOnce(i32) -> Option<u64>,
    ) {
        // A read while the parent is still in the tree is the attribution.
        // It has to survive a later snapshot that finds both gone, and it has
        // to move the total: the parent's child counter does not include a
        // zombie that has not been reaped yet. An identity this tree has
        // never seen counts only as the child of a parent that is still
        // present; a stray pid is not a member.
        let identity = (reading.pid, reading.started);
        // bsd_info can fail while the exit rusage is already published. The
        // stored parent identifies which lifetime to check, never whether it
        // is still present. Read its current start identity after the exit
        // sample; cached presence can predate the parent's disappearance.
        let stored_parent = self.entries.get(&identity).and_then(|entry| entry.parent);
        let parent_pid = if reading.parent < 0 {
            stored_parent.map(|id| id.0)
        } else {
            (reading.parent > 0).then_some(reading.parent)
        };
        let parent_now = parent_pid
            .and_then(|pid| current_start(pid).map(|started| (pid, started)))
            .filter(|id| {
                self.entries.contains_key(id) && (reading.parent >= 0 || Some(*id) == stored_parent)
            });
        if let Some(entry) = self.entries.get_mut(&identity) {
            raise_floor(&mut entry.reading, &reading);
            if parent_now.is_some() {
                if !entry.exit_evidenced {
                    entry.parent = parent_now;
                }
                entry.exit_evidenced = true;
            }
        } else if let Some(parent) = parent_now {
            self.entries.insert(
                identity,
                Entry {
                    reading,
                    parent: Some(parent),
                    missing: true,
                    reaped: false,
                    exit_evidenced: true,
                },
            );
        }
        self.total = self.reconcile_missing(false);
        self.upper = self.reconcile_missing(true);
    }

    pub fn bounds(&self) -> ((u64, u64), (u64, u64)) {
        ((self.total.0, self.upper.0), (self.total.1, self.upper.1))
    }

    pub fn publish_bounds(&self, report: &mut crate::report::Report) {
        if self.totals().is_none() {
            let ((user_lower, user_upper), (kernel_lower, kernel_upper)) = self.bounds();
            let ms = |ns: u64| ns as f64 / 1_000_000.0;
            report.cpu_user_ms_bounds = Some(crate::report::CpuBounds {
                lower: ms(user_lower),
                upper: ms(user_upper),
            });
            report.cpu_kernel_ms_bounds = Some(crate::report::CpuBounds {
                lower: ms(kernel_lower),
                upper: ms(kernel_upper),
            });
        }
    }

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
                exit_evidenced: false,
            });
            // Reparented live descendants were orphaned, not reaped into
            // the old parent. An exit read already attributed this lifetime
            // to the parent that was present; a later ppid must not drop it.
            if reading.parent >= 0
                && entry.reading.parent != reading.parent
                && !entry.exit_evidenced
            {
                entry.parent = parent;
            }
            entry.missing = false;
            entry.reaped = false;
            // Counters belong to this lifetime and cannot decrease, including
            // an exit floor whose parent attribution remains unproved.
            let floor = entry.reading.clone();
            entry.reading = reading;
            raise_floor(&mut entry.reading, &floor);
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
        self.ambiguous |= self.entries.values().any(|entry| {
            entry.missing && !entry.reaped && !entry.exit_evidenced && entry.parent.is_some()
        });
        self.total = self.reconcile_missing(false);
        self.upper = self.reconcile_missing(true);
    }

    pub fn totals(&self) -> Option<(u64, u64)> {
        (!self.ambiguous).then_some(self.total)
    }

    fn reconcile_missing(&self, upper: bool) -> (u64, u64) {
        // An unresolved edge may overlap the parent's child counter (lower)
        // or remain independent after orphaning (upper). Never pick a total
        // from that interval. Evidenced reaped edges deduplicate in both folds.
        let folded_parent = |entry: &Entry| {
            entry
                .parent
                .filter(|_| entry.missing && (!upper || entry.reaped || entry.exit_evidenced))
        };
        // Fold leaves upward without recursion or repeated descendant scans.
        let mut pending: BTreeMap<_, _> = self
            .entries
            .keys()
            .map(|id| (*id, PendingEntry::default()))
            .collect();
        for entry in self.entries.values() {
            if let Some(parent) = folded_parent(entry) {
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
            let parent = folded_parent(entry);
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

fn raise_floor(reading: &mut CpuReading, floor: &CpuReading) {
    reading.user = reading.user.max(floor.user);
    reading.kernel = reading.kernel.max(floor.kernel);
    reading.child_user = reading.child_user.max(floor.child_user);
    reading.child_kernel = reading.child_kernel.max(floor.child_kernel);
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
    fn exit_samples_preserve_three_generation_cpu_without_periodic_sweeps() {
        let mut tree = TreeCpu::default();
        tree.observe(vec![
            reading(1, 0, 10, 0),
            reading(2, 1, 20, 0),
            reading(3, 2, 100, 0),
        ]);
        tree.record_exit(reading(3, 2, 110, 0), |pid| Some(pid as u64));
        // The next exit snapshot observes the parent's reaped-child counters.
        tree.observe(vec![reading(1, 0, 10, 0), reading(2, 1, 40, 110)]);
        tree.observe(vec![reading(1, 0, 20, 150)]);
        assert_eq!(tree.totals(), Some((170, 85)));
        tree.observe(vec![]);
        assert_eq!(tree.totals(), Some((170, 85)));
    }

    #[test]
    fn exit_read_with_parent_present_survives_both_generations_vanishing() {
        let mut tree = TreeCpu::default();
        tree.observe(vec![
            reading(1, 0, 10, 0),
            reading(2, 1, 20, 0),
            reading(3, 2, 100, 0),
        ]);
        tree.record_exit(reading(3, 2, 110, 0), |pid| Some(pid as u64));
        // The zombie is still in the next snapshot, so nothing is missing yet.
        // The parent then reaps and exits before any snapshot sees it alone.
        let mut zombie = reading(3, 2, 110, 0);
        tree.observe(vec![
            reading(1, 0, 10, 0),
            reading(2, 1, 20, 0),
            zombie.clone(),
        ]);
        assert_eq!(tree.totals(), Some((140, 70)));
        tree.observe(vec![reading(1, 0, 20, 150)]);
        assert_eq!(tree.totals(), Some((170, 85)));
        assert!(!tree.ambiguous);
        // A lower later read of the same lifetime must not drop the exit floor.
        zombie.user = 100;
        zombie.kernel = 50;
        tree.observe(vec![reading(1, 0, 20, 150), zombie]);
        tree.observe(vec![reading(1, 0, 20, 150)]);
        assert_eq!(tree.totals(), Some((170, 85)));
    }

    #[test]
    fn exit_read_without_a_ppid_keeps_the_stored_parent() {
        let mut tree = TreeCpu::default();
        tree.observe(vec![
            reading(1, 0, 10, 0),
            reading(2, 1, 20, 0),
            reading(3, 2, 100, 0),
        ]);
        tree.record_exit(reading(3, -1, 110, 0), |pid| Some(pid as u64));
        tree.record_exit(reading(2, -1, 40, 110), |pid| Some(pid as u64));
        tree.observe(vec![reading(1, 0, 20, 150)]);
        assert_eq!(tree.totals(), Some((170, 85)));
        tree.observe(vec![]);
        assert_eq!(tree.totals(), Some((170, 85)));
    }

    #[test]
    fn exit_without_parent_metadata_cannot_use_a_tracked_replacement_parent() {
        let mut tree = TreeCpu::default();
        tree.observe(vec![
            reading(1, 0, 10, 0),
            reading(2, 1, 20, 0),
            reading(3, 2, 100, 0),
        ]);
        let mut replacement = reading(2, 1, 40, 0);
        replacement.started = 99;
        tree.observe(vec![
            reading(1, 0, 20, 150),
            replacement,
            reading(3, -1, 100, 0),
        ]);
        tree.record_exit(reading(3, -1, 110, 0), |pid| {
            assert_eq!(pid, 2);
            Some(99)
        });
        tree.observe(vec![reading(1, 0, 20, 150)]);
        assert_eq!(tree.totals(), None);
        // The sampled lifetimes sum to 20 + 20 + 110 + 40 = 190.
        // Counting the orphan independently of the root's child counter
        // instead permits 20 + 150 + 110 = 280.
        assert_eq!(tree.bounds(), ((190, 280), (95, 140)));
    }

    #[test]
    fn reaped_exit_with_the_parent_still_readable_needs_no_zombie_sample() {
        let mut tree = TreeCpu::default();
        tree.observe(vec![
            reading(1, 0, 10, 0),
            reading(2, 1, 20, 0),
            reading(3, 2, 100, 0),
        ]);
        // waitpid already reaped the grandchild. The parent is still alive,
        // so its child counter is the attribution. No exit sample of pid 3.
        tree.observe(vec![reading(1, 0, 10, 0), reading(2, 1, 40, 110)]);
        assert_eq!(tree.totals(), Some((160, 80)));
        tree.observe(vec![reading(1, 0, 20, 150)]);
        assert_eq!(tree.totals(), Some((170, 85)));
        tree.observe(vec![]);
        assert_eq!(tree.totals(), Some((170, 85)));
    }

    #[test]
    fn exit_reads_between_sweeps_stay_exact_when_only_the_root_remains() {
        let mut tree = TreeCpu::default();
        tree.observe(vec![
            reading(1, 0, 10, 0),
            reading(2, 1, 20, 0),
            reading(3, 2, 100, 0),
        ]);
        tree.record_exit(reading(3, 2, 110, 0), |pid| Some(pid as u64));
        tree.record_exit(reading(2, 1, 40, 110), |pid| Some(pid as u64));
        tree.observe(vec![reading(1, 0, 20, 150)]);
        assert_eq!(tree.totals(), Some((170, 85)));
    }

    #[test]
    fn unseen_child_exit_while_parent_lives_is_in_the_total() {
        let mut tree = TreeCpu::default();
        tree.observe(vec![reading(1, 0, 10, 0), reading(2, 1, 20, 0)]);
        assert_eq!(tree.totals(), Some((30, 15)));
        // Forked and exited between sweeps. The parent has not reaped it, so
        // the parent's child counter is still 0; the exit sample is the total.
        tree.record_exit(reading(3, 2, 100, 0), |pid| Some(pid as u64));
        assert_eq!(tree.totals(), Some((130, 65)));
        tree.observe(vec![reading(1, 0, 10, 0), reading(2, 1, 20, 0)]);
        assert_eq!(tree.totals(), Some((130, 65)));
        // The root's child counter (40) is less than the exit sample folded
        // through the parent (120). The published total follows the sample.
        tree.observe(vec![reading(1, 0, 20, 40)]);
        assert_eq!(tree.totals(), Some((140, 70)));
    }

    #[test]
    fn stray_exit_and_reused_pid_do_not_replace_a_tracked_lifetime() {
        let mut tree = TreeCpu::default();
        tree.observe(vec![
            reading(1, 0, 10, 0),
            reading(2, 1, 20, 0),
            reading(3, 2, 100, 0),
        ]);
        let mut stray = reading(9, 8, 500, 0);
        stray.started = 9;
        tree.record_exit(stray, |pid| Some(pid as u64));
        assert_eq!(tree.totals(), Some((130, 65)));
        let mut reused = reading(3, 2, 900, 0);
        reused.started = 99;
        tree.record_exit(reused, |pid| Some(pid as u64));
        // The new lifetime is its own exit, folded beside the original 100,
        // not a replacement of it (that would total 930).
        tree.observe(vec![
            reading(1, 0, 10, 0),
            reading(2, 1, 20, 0),
            reading(3, 2, 100, 0),
        ]);
        assert_eq!(tree.totals(), Some((1030, 515)));
    }

    #[test]
    fn exit_after_the_parent_is_already_gone_does_not_invent_attribution() {
        let mut tree = TreeCpu::default();
        tree.observe(vec![
            reading(1, 0, 10, 0),
            reading(2, 1, 20, 0),
            reading(3, 2, 100, 0),
        ]);
        tree.observe(vec![reading(1, 0, 20, 150)]);
        assert_eq!(tree.totals(), None);
        tree.record_exit(reading(3, 2, 110, 0), |_| None);
        assert_eq!(tree.totals(), None);
        // The floor moves the upper bound. It does not make the total exact.
        assert_eq!(tree.bounds(), ((170, 280), (85, 140)));
    }

    #[test]
    fn exit_before_presence_refresh_does_not_certify_a_vanished_parent() {
        for (exit_parent, parent_start) in [(-1, None), (2, None), (-1, Some(99)), (2, Some(99))] {
            let mut tree = TreeCpu::default();
            tree.observe(vec![
                reading(1, 0, 10, 0),
                reading(2, 1, 20, 0),
                reading(3, 2, 100, 0),
            ]);
            // The parent exits without reaping the grandchild and is reaped
            // by the root. No sweep has refreshed its cached presence yet.
            tree.record_exit(reading(3, exit_parent, 110, 0), |pid| {
                assert_eq!(pid, 2);
                parent_start
            });
            tree.observe(vec![reading(1, 0, 20, 150)]);
            assert_eq!(tree.totals(), None);
            assert_eq!(tree.bounds(), ((170, 280), (85, 140)));
            // A later, lower read with unavailable parent metadata must not
            // erase the exit floor or resolve the unknown reaping history.
            tree.observe(vec![reading(1, 0, 20, 150), reading(3, -1, 100, 0)]);
            assert_eq!(tree.totals(), None);
            assert_eq!(tree.bounds(), ((280, 280), (140, 140)));
            tree.observe(vec![reading(1, 0, 20, 150)]);
            assert_eq!(tree.totals(), None);
            assert_eq!(tree.bounds(), ((170, 280), (85, 140)));
        }
    }

    #[test]
    fn indistinguishable_nested_reaping_and_orphan_tail_have_bounds() {
        let mut tree = TreeCpu::default();
        tree.observe(vec![
            reading(1, 0, 10, 0),
            reading(2, 1, 20, 0),
            reading(3, 2, 100, 0),
        ]);
        tree.observe(vec![reading(1, 0, 20, 150)]);
        assert_eq!(tree.totals(), None);
        assert_eq!(tree.bounds(), ((170, 270), (85, 135)));
        tree.observe(vec![reading(1, 0, 30, 150)]);
        assert_eq!(tree.totals(), None);
        assert_eq!(tree.bounds(), ((180, 280), (90, 140)));
    }

    #[test]
    fn ambiguous_bounds_are_serialized_in_milliseconds_without_cpu_totals() {
        let spec = crate::cli::parse(
            [
                "run",
                "--mem-mb",
                "512",
                "--timeout-ms",
                "1000",
                "--out",
                "result.json",
                "--",
                "fixture",
            ]
            .map(Into::into)
            .to_vec(),
        )
        .unwrap();
        let mut report = crate::report::Report::new(&spec, "macos-phys-footprint");
        let mut tree = TreeCpu::default();
        let scaled = |pid, parent, user, children| {
            let mut r = reading(pid, parent, user, children);
            r.user *= 1_000_000;
            r.kernel *= 1_000_000;
            r.child_user *= 1_000_000;
            r.child_kernel *= 1_000_000;
            r
        };
        tree.observe(vec![
            scaled(1, 0, 10, 0),
            scaled(2, 1, 20, 0),
            scaled(3, 2, 100, 0),
        ]);
        tree.publish_bounds(&mut report);
        let exact = serde_json::to_value(&report).unwrap();
        assert!(exact.get("cpuUserMsBounds").is_none());
        assert!(exact.get("cpuKernelMsBounds").is_none());
        tree.observe(vec![scaled(1, 0, 20, 150)]);
        tree.publish_bounds(&mut report);
        let ambiguous = serde_json::to_value(&report).unwrap();
        assert!(ambiguous["cpuUserMs"].is_null());
        assert!(ambiguous["cpuKernelMs"].is_null());
        assert_eq!(
            ambiguous["cpuUserMsBounds"],
            serde_json::json!({"lower":170.0,"upper":270.0})
        );
        assert_eq!(
            ambiguous["cpuKernelMsBounds"],
            serde_json::json!({"lower":85.0,"upper":135.0})
        );
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
