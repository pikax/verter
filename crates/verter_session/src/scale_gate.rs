//! Shared assertions for the gate versions of the nightly scale tier.
//!
//! A full-size scale test in a `nightly_scale` module keeps its size; its
//! gate version runs the same workload at three small sizes with equal steps
//! and reads the engine's deterministic work counters. When every counter
//! grows by exactly the same amount for each step, the work is affine in the
//! size: any term that grows faster than the size, however small, changes the
//! second step. The affine counters then give the exact counts the full-size
//! test would measure, so the gate version can apply the full-size test's own
//! bounds and growth checks to them.

/// One counter's value at each of the three gate sizes.
pub(crate) struct Counter<'a> {
    pub(crate) name: &'a str,
    pub(crate) values: [u64; 3],
}

/// Affine work over three equally spaced sizes.
pub(crate) struct AffineWork<'a> {
    sizes: [usize; 3],
    counters: Vec<Counter<'a>>,
}

impl<'a> AffineWork<'a> {
    /// Requires equal size steps and, for every counter, the same increment
    /// over both steps. `label` names the workload in failures.
    pub(crate) fn new(label: &str, sizes: [usize; 3], counters: Vec<Counter<'a>>) -> Self {
        assert!(
            sizes[0] < sizes[1] && sizes[1] - sizes[0] == sizes[2] - sizes[1],
            "{label}: gate sizes must be equally spaced, got {sizes:?}"
        );
        for Counter { name, values } in &counters {
            let [small, middle, large] = *values;
            assert!(
                middle.checked_sub(small) == large.checked_sub(middle) && small <= middle,
                "{label}: {name} must grow by the same amount for each step of {} \
                 (no term may grow faster than the size): {values:?} at {sizes:?}",
                sizes[1] - sizes[0],
            );
        }
        Self { sizes, counters }
    }

    /// The count `name` reaches at `size` on the affine line, which is the
    /// count the full-size workload measures. `size` must lie a whole number
    /// of steps from the first gate size, so the count is exact.
    pub(crate) fn at(&self, name: &str, size: usize) -> u64 {
        let counter = self
            .counters
            .iter()
            .find(|counter| counter.name == name)
            .unwrap_or_else(|| panic!("no counter named {name}"));
        let step = self.sizes[1] - self.sizes[0];
        let first = self.sizes[0];
        assert!(
            size >= first && (size - first).is_multiple_of(step),
            "{size} must be a whole number of {step}-steps above {first}"
        );
        let per_step = counter.values[1] - counter.values[0];
        counter.values[0] + per_step * ((size - first) / step) as u64
    }

    /// Every counter at `size`, in the order given.
    pub(crate) fn all_at(&self, size: usize) -> Vec<(&'a str, u64)> {
        self.counters
            .iter()
            .map(|counter| (counter.name, self.at(counter.name, size)))
            .collect()
    }
}
