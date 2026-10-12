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
    pub exited: bool,
    pub user: u64,
    pub kernel: u64,
    pub child_user: u64,
    pub child_kernel: u64,
}

impl CpuReading {
    /// Normalize platform ticks before any accounting or report publication.
    pub fn normalize_timebase(&mut self, numer: u32, denom: u32) {
        let ns = |ticks: u64| {
            let value = u128::from(ticks) * u128::from(numer) / u128::from(denom);
            u64::try_from(value).unwrap_or(u64::MAX)
        };
        self.user = ns(self.user);
        self.kernel = ns(self.kernel);
        self.child_user = ns(self.child_user);
        self.child_kernel = ns(self.child_kernel);
    }
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
    upper: (u64, u64),
    ambiguous: bool,
}

impl TreeCpu {
    pub fn record_exit(
        &mut self,
        reading: CpuReading,
        current_parent: impl FnOnce(i32) -> Option<CpuReading>,
    ) {
        // Exit counters are a lifetime floor, not evidence of reaping. A live
        // parent can exit later without waiting for this child. An identity
        // first seen here counts only with a verified parent in the tree.
        let identity = (reading.pid, reading.started);
        // bsd_info can fail while the exit rusage is already published. The
        // stored parent identifies which lifetime to check, never whether it
        // is still alive. Read its current lifetime after the exit sample;
        // cached presence can predate the parent's exit. Readable zombies
        // retain their start identity but have already orphaned live children.
        let stored_parent = self.entries.get(&identity).and_then(|entry| entry.parent);
        let parent_pid = if reading.parent < 0 {
            stored_parent.map(|id| id.0)
        } else {
            (reading.parent > 0).then_some(reading.parent)
        };
        let parent_now = parent_pid
            .and_then(|pid| {
                current_parent(pid)
                    .filter(|parent| parent.pid == pid && !parent.exited)
                    .map(|parent| (pid, parent.started))
            })
            .filter(|id| {
                self.entries.contains_key(id) && (reading.parent >= 0 || Some(*id) == stored_parent)
            });
        if let Some(entry) = self.entries.get_mut(&identity) {
            raise_floor(&mut entry.reading, &reading);
        } else if let Some(parent) = parent_now {
            self.entries.insert(
                identity,
                Entry {
                    reading,
                    parent: Some(parent),
                    missing: false,
                    reaped: false,
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

    pub fn observe(
        &mut self,
        readings: Vec<CpuReading>,
        mut current_parent: impl FnMut(&CpuReading, i32) -> Option<CpuReading>,
    ) {
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
            // the old parent. An exit read does not establish reaping and
            // must not prevent later parent metadata from revealing an orphan.
            if reading.parent >= 0 && entry.reading.parent != reading.parent {
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
        let mut verified_parents = Vec::new();
        for entry in self.entries.values_mut() {
            if entry.missing && !entry.reaped {
                if let Some(id) = entry.parent {
                    // The batch is sequential: its parent sample may predate
                    // this child's disappearance. Verify the parent's living
                    // identity now, after confirming disappearance.
                    if let Some(parent) = current_parent(&entry.reading, id.0)
                        .filter(|parent| (parent.pid, parent.started) == id && !parent.exited)
                    {
                        entry.reaped = true;
                        verified_parents.push((id, parent));
                    }
                }
            }
        }
        for (id, parent) in verified_parents {
            // Retain the tail newly included by this post-reaping read. The
            // earlier batch's counters may predate waitpid's accounting.
            raise_floor(
                &mut self.entries.get_mut(&id).expect("tracked parent").reading,
                &parent,
            );
        }
        // Only a living parent verified after disappearance can attribute it to
        // its own child accounting. When both generations vanish, aggregate
        // counters cannot distinguish nested reaping from an orphan plus an
        // independent parent/sibling tail. Later samples cannot repair this
        // missing attribution, so never publish a numeric total again.
        self.ambiguous |= self
            .entries
            .values()
            .any(|entry| entry.missing && !entry.reaped && entry.parent.is_some());
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
                .filter(|_| entry.missing && (!upper || entry.reaped))
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
    reading.exited |= floor.exited;
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
            exited: false,
            user,
            kernel: user / 2,
            child_user,
            child_kernel: child_user / 2,
        }
    }
    // Most fixtures model a completed child lookup followed by a fresh parent
    // lookup with unchanged counters. Race tests supply the later read explicitly.
    fn observe(tree: &mut TreeCpu, readings: Vec<CpuReading>) {
        let later = readings.clone();
        tree.observe(readings, |_, pid| {
            later.iter().find(|r| r.pid == pid).cloned()
        });
    }
    #[test]
    fn reaped_children_replace_samples_without_double_counting() {
        let mut tree = TreeCpu::default();
        observe(&mut tree, vec![reading(1, 0, 10, 0), reading(2, 1, 100, 0)]);
        assert_eq!(tree.totals(), Some((110, 55)));
        // Reaping also captures the tail since the last child sample.
        tree.observe(vec![reading(1, 0, 20, 100)], |_, _| {
            Some(reading(1, 0, 20, 120))
        });
        assert_eq!(tree.totals(), Some((140, 70)));
        observe(&mut tree, vec![reading(1, 0, 30, 120)]);
        assert_eq!(tree.totals(), Some((150, 75)));
    }
    #[test]
    fn nested_reaping_requires_observed_attribution() {
        for intermediate_sample in [false, true] {
            let mut tree = TreeCpu::default();
            observe(
                &mut tree,
                vec![
                    reading(1, 0, 10, 0),
                    reading(2, 1, 20, 0),
                    reading(3, 2, 100, 0),
                ],
            );
            assert_eq!(tree.totals(), Some((130, 65)));
            if intermediate_sample {
                observe(
                    &mut tree,
                    vec![reading(1, 0, 10, 0), reading(2, 1, 30, 110)],
                );
                assert_eq!(tree.totals(), Some((150, 75)));
            }
            // With the intermediate observation the root's child counter
            // covers the already attributed subtree, including its unseen
            // tails. Without it, an orphan history is indistinguishable.
            observe(&mut tree, vec![reading(1, 0, 20, 150)]);
            assert_eq!(tree.totals(), intermediate_sample.then_some((170, 85)));
            observe(&mut tree, vec![reading(1, 0, 30, 150)]);
            assert_eq!(tree.totals(), intermediate_sample.then_some((180, 90)));
        }
    }

    #[test]
    fn exit_samples_preserve_three_generation_cpu_without_periodic_sweeps() {
        let mut tree = TreeCpu::default();
        observe(
            &mut tree,
            vec![
                reading(1, 0, 10, 0),
                reading(2, 1, 20, 0),
                reading(3, 2, 100, 0),
            ],
        );
        tree.record_exit(reading(3, 2, 110, 0), |pid| Some(reading(pid, 0, 0, 0)));
        // The next exit snapshot observes the parent's reaped-child counters.
        observe(
            &mut tree,
            vec![reading(1, 0, 10, 0), reading(2, 1, 40, 110)],
        );
        observe(&mut tree, vec![reading(1, 0, 20, 150)]);
        assert_eq!(tree.totals(), Some((170, 85)));
        observe(&mut tree, vec![]);
        assert_eq!(tree.totals(), Some((170, 85)));
    }

    #[test]
    fn exit_floor_survives_without_certifying_both_generations_vanishing() {
        let mut tree = TreeCpu::default();
        observe(
            &mut tree,
            vec![
                reading(1, 0, 10, 0),
                reading(2, 1, 20, 0),
                reading(3, 2, 100, 0),
            ],
        );
        tree.record_exit(reading(3, 2, 110, 0), |pid| Some(reading(pid, 0, 0, 0)));
        // The zombie is still in the next snapshot, so nothing is missing yet.
        // The parent then reaps and exits before any snapshot sees it alone.
        let mut zombie = reading(3, 2, 110, 0);
        zombie.exited = true;
        observe(
            &mut tree,
            vec![reading(1, 0, 10, 0), reading(2, 1, 20, 0), zombie.clone()],
        );
        assert_eq!(tree.totals(), Some((140, 70)));
        observe(&mut tree, vec![reading(1, 0, 20, 150)]);
        assert_eq!(tree.totals(), None);
        assert_eq!(tree.bounds(), ((170, 280), (85, 140)));
        // A lower later read of the same lifetime must not drop the exit floor.
        zombie.user = 100;
        zombie.kernel = 50;
        observe(&mut tree, vec![reading(1, 0, 20, 150), zombie]);
        observe(&mut tree, vec![reading(1, 0, 20, 150)]);
        assert_eq!(tree.totals(), None);
        assert_eq!(tree.bounds(), ((170, 280), (85, 140)));
    }

    #[test]
    fn exit_read_without_a_ppid_keeps_the_stored_parent() {
        let mut tree = TreeCpu::default();
        observe(
            &mut tree,
            vec![
                reading(1, 0, 10, 0),
                reading(2, 1, 20, 0),
                reading(3, 2, 100, 0),
            ],
        );
        tree.record_exit(reading(3, -1, 110, 0), |pid| Some(reading(pid, 0, 0, 0)));
        // A later disappearance, followed by a live-parent read, establishes
        // reaping independently of the unavailable exit ppid.
        observe(
            &mut tree,
            vec![reading(1, 0, 10, 0), reading(2, 1, 40, 110)],
        );
        tree.record_exit(reading(2, -1, 40, 110), |pid| Some(reading(pid, 0, 0, 0)));
        observe(&mut tree, vec![reading(1, 0, 20, 150)]);
        assert_eq!(tree.totals(), Some((170, 85)));
        observe(&mut tree, vec![]);
        assert_eq!(tree.totals(), Some((170, 85)));
    }

    #[test]
    fn exit_without_parent_metadata_cannot_use_a_tracked_replacement_parent() {
        let mut tree = TreeCpu::default();
        observe(
            &mut tree,
            vec![
                reading(1, 0, 10, 0),
                reading(2, 1, 20, 0),
                reading(3, 2, 100, 0),
            ],
        );
        let mut replacement = reading(2, 1, 40, 0);
        replacement.started = 99;
        observe(
            &mut tree,
            vec![reading(1, 0, 20, 150), replacement, reading(3, -1, 100, 0)],
        );
        tree.record_exit(reading(3, -1, 110, 0), |pid| {
            assert_eq!(pid, 2);
            let mut parent = reading(pid, 0, 0, 0);
            parent.started = 99;
            Some(parent)
        });
        observe(&mut tree, vec![reading(1, 0, 20, 150)]);
        assert_eq!(tree.totals(), None);
        // The sampled lifetimes sum to 20 + 20 + 110 + 40 = 190.
        // Counting the orphan independently of the root's child counter
        // instead permits 20 + 150 + 110 = 280.
        assert_eq!(tree.bounds(), ((190, 280), (95, 140)));
    }

    #[test]
    fn reaped_exit_with_the_parent_still_readable_needs_no_zombie_sample() {
        let mut tree = TreeCpu::default();
        observe(
            &mut tree,
            vec![
                reading(1, 0, 10, 0),
                reading(2, 1, 20, 0),
                reading(3, 2, 100, 0),
            ],
        );
        // waitpid already reaped the grandchild. The parent is still alive,
        // so its child counter is the attribution. No exit sample of pid 3.
        observe(
            &mut tree,
            vec![reading(1, 0, 10, 0), reading(2, 1, 40, 110)],
        );
        assert_eq!(tree.totals(), Some((160, 80)));
        observe(&mut tree, vec![reading(1, 0, 20, 150)]);
        assert_eq!(tree.totals(), Some((170, 85)));
        observe(&mut tree, vec![]);
        assert_eq!(tree.totals(), Some((170, 85)));
    }

    #[test]
    fn exit_reads_between_sweeps_stay_exact_when_only_the_root_remains() {
        let mut tree = TreeCpu::default();
        observe(
            &mut tree,
            vec![
                reading(1, 0, 10, 0),
                reading(2, 1, 20, 0),
                reading(3, 2, 100, 0),
            ],
        );
        tree.record_exit(reading(3, 2, 110, 0), |pid| Some(reading(pid, 0, 0, 0)));
        // This event-triggered read observes reaping before the parent exits.
        observe(
            &mut tree,
            vec![reading(1, 0, 10, 0), reading(2, 1, 40, 110)],
        );
        tree.record_exit(reading(2, 1, 40, 110), |pid| Some(reading(pid, 0, 0, 0)));
        observe(&mut tree, vec![reading(1, 0, 20, 150)]);
        assert_eq!(tree.totals(), Some((170, 85)));
    }

    #[test]
    fn unseen_child_exit_while_parent_lives_is_in_the_total() {
        let mut tree = TreeCpu::default();
        observe(&mut tree, vec![reading(1, 0, 10, 0), reading(2, 1, 20, 0)]);
        assert_eq!(tree.totals(), Some((30, 15)));
        // Forked and exited between sweeps. The parent has not reaped it, so
        // the parent's child counter is still 0; the exit sample is the total.
        tree.record_exit(reading(3, 2, 100, 0), |pid| Some(reading(pid, 0, 0, 0)));
        assert_eq!(tree.totals(), Some((130, 65)));
        observe(&mut tree, vec![reading(1, 0, 10, 0), reading(2, 1, 20, 0)]);
        assert_eq!(tree.totals(), Some((130, 65)));
        // A disappearance followed by the living root's read proves reaping.
        // Its child counter (40) is below the 120 sampled through the parent,
        // so the published total retains that floor.
        observe(&mut tree, vec![reading(1, 0, 20, 40)]);
        assert_eq!(tree.totals(), Some((140, 70)));
    }

    #[test]
    fn stray_exit_and_reused_pid_do_not_replace_a_tracked_lifetime() {
        let mut tree = TreeCpu::default();
        observe(
            &mut tree,
            vec![
                reading(1, 0, 10, 0),
                reading(2, 1, 20, 0),
                reading(3, 2, 100, 0),
            ],
        );
        let mut stray = reading(9, 8, 500, 0);
        stray.started = 9;
        tree.record_exit(stray, |pid| Some(reading(pid, 0, 0, 0)));
        assert_eq!(tree.totals(), Some((130, 65)));
        let mut reused = reading(3, 2, 900, 0);
        reused.started = 99;
        tree.record_exit(reused, |pid| Some(reading(pid, 0, 0, 0)));
        // The new lifetime is its own exit, folded beside the original 100,
        // not a replacement of it (that would total 930).
        observe(
            &mut tree,
            vec![
                reading(1, 0, 10, 0),
                reading(2, 1, 20, 0),
                reading(3, 2, 100, 0),
            ],
        );
        assert_eq!(tree.totals(), Some((1030, 515)));
    }

    #[test]
    fn exit_after_the_parent_is_already_gone_does_not_invent_attribution() {
        let mut tree = TreeCpu::default();
        observe(
            &mut tree,
            vec![
                reading(1, 0, 10, 0),
                reading(2, 1, 20, 0),
                reading(3, 2, 100, 0),
            ],
        );
        observe(&mut tree, vec![reading(1, 0, 20, 150)]);
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
            observe(
                &mut tree,
                vec![
                    reading(1, 0, 10, 0),
                    reading(2, 1, 20, 0),
                    reading(3, 2, 100, 0),
                ],
            );
            // The parent exits without reaping the grandchild and is reaped
            // by the root. No sweep has refreshed its cached presence yet.
            tree.record_exit(reading(3, exit_parent, 110, 0), |pid| {
                assert_eq!(pid, 2);
                parent_start.map(|started| {
                    let mut parent = reading(pid, 0, 0, 0);
                    parent.started = started;
                    parent
                })
            });
            observe(&mut tree, vec![reading(1, 0, 20, 150)]);
            assert_eq!(tree.totals(), None);
            assert_eq!(tree.bounds(), ((170, 280), (85, 140)));
            // A later, lower read with unavailable parent metadata must not
            // erase the exit floor or resolve the unknown reaping history.
            observe(
                &mut tree,
                vec![reading(1, 0, 20, 150), reading(3, -1, 100, 0)],
            );
            assert_eq!(tree.totals(), None);
            assert_eq!(tree.bounds(), ((280, 280), (140, 140)));
            observe(&mut tree, vec![reading(1, 0, 20, 150)]);
            assert_eq!(tree.totals(), None);
            assert_eq!(tree.bounds(), ((170, 280), (85, 140)));
        }
    }

    #[test]
    fn exit_with_readable_zombie_parent_retains_orphan_bounds() {
        for (sample_parent_exit, root_children, lower, upper) in
            [(false, 150, 170, 280), (true, 260, 280, 390)]
        {
            let mut tree = TreeCpu::default();
            observe(
                &mut tree,
                vec![
                    reading(1, 0, 10, 0),
                    reading(2, 1, 20, 0),
                    reading(3, 2, 100, 0),
                ],
            );
            // The parent exits without reaping the grandchild. Its zombie
            // remains readable with the same start identity until root reaps it.
            let mut parent = reading(2, 1, 150, 0);
            parent.exited = true;
            if sample_parent_exit {
                tree.record_exit(parent.clone(), |pid| Some(reading(pid, 0, 0, 0)));
            }
            tree.record_exit(reading(3, -1, 110, 0), |pid| {
                assert_eq!(pid, 2);
                Some(parent)
            });
            // 260 includes an unrelated reaped sibling's 110 units; that
            // counter cannot identify or absorb the orphan's independent CPU.
            observe(&mut tree, vec![reading(1, 0, 20, root_children)]);
            assert_eq!(tree.totals(), None);
            assert_eq!(tree.bounds(), ((lower, upper), (lower / 2, upper / 2)));
        }
    }

    #[test]
    fn zombie_parent_snapshot_cannot_attribute_a_missing_child() {
        let mut tree = TreeCpu::default();
        observe(
            &mut tree,
            vec![
                reading(1, 0, 10, 0),
                reading(2, 1, 20, 0),
                reading(3, 2, 100, 0),
            ],
        );
        let mut parent = reading(2, 1, 150, 0);
        parent.exited = true;
        // Exit metadata for the orphan is unavailable; neither a failed
        // callback nor the subsequent readable zombie proves reaping.
        tree.record_exit(reading(3, -1, 110, 0), |_| None);
        observe(&mut tree, vec![reading(1, 0, 20, 0), parent]);
        assert_eq!(tree.totals(), None);
        observe(&mut tree, vec![reading(1, 0, 20, 260)]);
        assert_eq!(tree.totals(), None);
        assert_eq!(tree.bounds(), ((280, 390), (140, 195)));
    }

    #[test]
    fn earlier_parent_sample_cannot_certify_a_later_child_disappearance() {
        for later_parent in [
            None,
            Some((2, 2, true)),
            Some((2, 99, false)),
            Some((9, 2, false)),
        ] {
            let mut tree = TreeCpu::default();
            observe(
                &mut tree,
                vec![
                    reading(1, 0, 10, 0),
                    reading(2, 1, 20, 0),
                    reading(3, 2, 100, 0),
                ],
            );
            // The sequential sweep reads the living parent first. It then exits
            // without waiting, and the orphan is reaped before the child's read.
            tree.observe(
                vec![reading(1, 0, 10, 0), reading(2, 1, 150, 0)],
                |child, parent_pid| {
                    assert_eq!((child.pid, child.started, parent_pid), (3, 3, 2));
                    later_parent.map(|(pid, started, exited)| {
                        let mut parent = reading(pid, 1, 150, 0);
                        parent.started = started;
                        parent.exited = exited;
                        parent
                    })
                },
            );
            // Root reaps the parent and an unseen sibling with 110 units of CPU.
            observe(&mut tree, vec![reading(1, 0, 20, 260)]);
            assert_eq!(tree.totals(), None);
            assert_eq!(tree.bounds(), ((280, 380), (140, 190)));
        }
    }

    #[test]
    fn living_parent_at_exit_does_not_prove_it_later_reaped_the_child() {
        let mut tree = TreeCpu::default();
        observe(
            &mut tree,
            vec![
                reading(1, 0, 10, 0),
                reading(2, 1, 20, 0),
                reading(3, 2, 100, 0),
            ],
        );
        tree.record_exit(reading(3, 2, 110, 0), |pid| Some(reading(pid, 1, 20, 0)));
        // The verified parent subsequently exits without waiting for its
        // zombie child. Root also reaps an unseen sibling using 110 units.
        observe(&mut tree, vec![reading(1, 0, 20, 260)]);
        assert_eq!(tree.totals(), None);
        assert_eq!(tree.bounds(), ((280, 390), (140, 195)));
    }

    #[test]
    fn indistinguishable_nested_reaping_and_orphan_tail_have_bounds() {
        let mut tree = TreeCpu::default();
        observe(
            &mut tree,
            vec![
                reading(1, 0, 10, 0),
                reading(2, 1, 20, 0),
                reading(3, 2, 100, 0),
            ],
        );
        observe(&mut tree, vec![reading(1, 0, 20, 150)]);
        assert_eq!(tree.totals(), None);
        assert_eq!(tree.bounds(), ((170, 270), (85, 135)));
        observe(&mut tree, vec![reading(1, 0, 30, 150)]);
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
            // ARM Darwin's 125/3 timebase: 24,000 ticks per millisecond.
            r.user *= 24_000;
            r.kernel *= 24_000;
            r.child_user *= 24_000;
            r.child_kernel *= 24_000;
            r.normalize_timebase(125, 3);
            r
        };
        observe(
            &mut tree,
            vec![
                scaled(1, 0, 10, 0),
                scaled(2, 1, 20, 0),
                scaled(3, 2, 100, 0),
            ],
        );
        assert_eq!(tree.totals(), Some((130_000_000, 65_000_000)));
        tree.publish_bounds(&mut report);
        let exact = serde_json::to_value(&report).unwrap();
        assert!(exact.get("cpuUserMsBounds").is_none());
        assert!(exact.get("cpuKernelMsBounds").is_none());
        observe(&mut tree, vec![scaled(1, 0, 20, 150)]);
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
                observe(
                    &mut tree,
                    vec![
                        reading(1, 0, 10, previous_children),
                        reading(2, 1, 20, 0),
                        reading(3, 2, 100, 0),
                    ],
                );
                assert_eq!(
                    tree.totals(),
                    Some((130 + previous_children, 65 + previous_children / 2))
                );
                // The parent and grandchild disappear together. A tail of 150
                // can be nested reaping, the parent's own CPU with an orphan,
                // or a smaller parent tail plus an unseen reaped sibling.
                // Neither counter magnitude nor earlier child CPU identifies
                // the reaped lifetimes. The user and kernel totals are unknown.
                observe(
                    &mut tree,
                    vec![reading(1, 0, 20, previous_children + child_tail)],
                );
                assert_eq!(tree.totals(), None);
                observe(
                    &mut tree,
                    vec![reading(1, 0, 30, previous_children + child_tail)],
                );
                assert_eq!(tree.totals(), None);
                // Later unrelated child CPU cannot resolve that history.
                observe(
                    &mut tree,
                    vec![reading(1, 0, 30, previous_children + child_tail + 120)],
                );
                assert_eq!(tree.totals(), None);
            }
        }
    }

    #[test]
    fn live_orphan_and_reused_parent_stay_outside_the_reaped_subtree() {
        let mut tree = TreeCpu::default();
        observe(
            &mut tree,
            vec![
                reading(1, 0, 10, 60),
                reading(2, 1, 20, 0),
                reading(3, 2, 100, 0),
            ],
        );
        let mut reused = reading(2, 1, 40, 0);
        reused.started = 99;
        observe(
            &mut tree,
            vec![reading(1, 0, 20, 90), reused, reading(3, 0, 140, 0)],
        );
        assert_eq!(tree.totals(), Some((290, 145)));
        observe(&mut tree, vec![reading(1, 0, 30, 140)]);
        assert_eq!(tree.totals(), Some((310, 155)));
    }
    #[test]
    fn unsampled_reaped_children_and_orphaned_samples_are_retained() {
        let mut tree = TreeCpu::default();
        observe(
            &mut tree,
            vec![reading(1, 0, 10, 60), reading(2, 1, 100, 0)],
        );
        observe(
            &mut tree,
            vec![reading(1, 0, 20, 60), reading(2, 0, 140, 0)],
        );
        observe(&mut tree, vec![reading(1, 0, 30, 60)]);
        assert_eq!(tree.totals(), Some((230, 115)));
    }
    #[test]
    fn reused_pids_do_not_replace_previous_lifetimes() {
        let mut tree = TreeCpu::default();
        observe(&mut tree, vec![reading(1, 0, 10, 0), reading(2, 1, 100, 0)]);
        observe(&mut tree, vec![reading(1, 0, 20, 100)]);
        let mut reused = reading(2, 1, 40, 0);
        reused.started = 99;
        observe(&mut tree, vec![reading(1, 0, 30, 100), reused]);
        assert_eq!(tree.totals(), Some((170, 85)));
    }
}
