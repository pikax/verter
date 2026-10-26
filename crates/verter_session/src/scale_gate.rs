//! Shared assertions for the gate versions of the nightly scale tier.
//!
//! A full-size scale test in a `nightly_scale` module keeps its size; its
//! gate version runs the same workload at small, equally spaced sizes and
//! reads the engine's deterministic work counters. When a counter's work is
//! a polynomial of a known degree in the size — affine for most workloads,
//! quadratic for one whose work is its length times its width — its finite
//! differences of the next order vanish: for affine work every step adds the
//! same amount, and any term that grows faster than the size, however small,
//! changes a later step. The polynomial then gives the exact count the
//! full-size test would measure, so the gate version can apply the full-size
//! test's own bounds and growth checks to it.

/// One counter's value at each gate size.
pub(crate) struct Counter<'a> {
    pub(crate) name: &'a str,
    pub(crate) values: Vec<u64>,
}

/// Work of a known polynomial degree over equally spaced sizes.
pub(crate) struct PolynomialWork<'a> {
    sizes: Vec<usize>,
    /// Per counter, its value and forward differences at the first size,
    /// up to the degree.
    leading: Vec<(&'a str, Vec<i128>)>,
}

/// Work affine in the size, from three equally spaced sizes: every counter
/// grows by the same amount for each step.
pub(crate) fn affine<'a>(
    label: &str,
    sizes: [usize; 3],
    counters: Vec<Counter<'a>>,
) -> PolynomialWork<'a> {
    PolynomialWork::new(label, 1, &sizes, counters)
}

/// Work quadratic in the size, from four equally spaced sizes: every
/// counter's step grows by the same amount for each step.
pub(crate) fn quadratic<'a>(
    label: &str,
    sizes: [usize; 4],
    counters: Vec<Counter<'a>>,
) -> PolynomialWork<'a> {
    PolynomialWork::new(label, 2, &sizes, counters)
}

fn forward_differences(values: &[u64]) -> Vec<Vec<i128>> {
    let mut rows = vec![values
        .iter()
        .map(|&value| i128::from(value))
        .collect::<Vec<_>>()];
    while rows.last().is_some_and(|row| row.len() > 1) {
        let row = rows.last().expect("a row");
        let next = row.windows(2).map(|pair| pair[1] - pair[0]).collect();
        rows.push(next);
    }
    rows
}

impl<'a> PolynomialWork<'a> {
    fn new(label: &str, degree: usize, sizes: &[usize], counters: Vec<Counter<'a>>) -> Self {
        assert_eq!(
            sizes.len(),
            degree + 2,
            "{label}: one size more than the degree needs"
        );
        let step = sizes[1] - sizes[0];
        assert!(
            step > 0 && sizes.windows(2).all(|pair| pair[1] - pair[0] == step),
            "{label}: gate sizes must be equally spaced, got {sizes:?}"
        );
        let leading = counters
            .into_iter()
            .map(|Counter { name, values }| {
                assert_eq!(
                    values.len(),
                    sizes.len(),
                    "{label}: {name} needs a value per size"
                );
                let rows = forward_differences(&values);
                assert!(
                    rows[degree + 1].iter().all(|&difference| difference == 0),
                    "{label}: {name} must grow as a polynomial of degree {degree} in the size \
                     (its differences of order {} must vanish; no term may grow faster): \
                     {values:?} at {sizes:?}",
                    degree + 1,
                );
                assert!(
                    rows[degree][0] >= 0,
                    "{label}: {name} must not shrink as the size grows: {values:?} at {sizes:?}"
                );
                (
                    name,
                    rows.iter().take(degree + 1).map(|row| row[0]).collect(),
                )
            })
            .collect();
        Self {
            sizes: sizes.to_vec(),
            leading,
        }
    }

    /// The count `name` reaches at `size` on its polynomial, which is the
    /// count the full-size workload measures. `size` must lie a whole number
    /// of steps from the first gate size, so the count is exact.
    pub(crate) fn at(&self, name: &str, size: usize) -> u64 {
        let (_, leading) = self
            .leading
            .iter()
            .find(|(counter, _)| *counter == name)
            .unwrap_or_else(|| panic!("no counter named {name}"));
        let step = self.sizes[1] - self.sizes[0];
        let first = self.sizes[0];
        assert!(
            size >= first && (size - first).is_multiple_of(step),
            "{size} must be a whole number of {step}-steps above {first}"
        );
        let steps = ((size - first) / step) as i128;
        // Newton's forward form: the sum of C(steps, j) times the j-th
        // difference at the first size.
        let mut binomial = 1i128;
        let mut count = 0i128;
        for (order, difference) in leading.iter().enumerate() {
            if order > 0 {
                binomial = binomial * (steps - order as i128 + 1) / order as i128;
            }
            count += binomial * difference;
        }
        u64::try_from(count).expect("a polynomial count at a larger size is not negative")
    }

    /// Every counter at `size`, in the order given.
    pub(crate) fn all_at(&self, size: usize) -> Vec<(&'a str, u64)> {
        self.leading
            .iter()
            .map(|(name, _)| (*name, self.at(name, size)))
            .collect()
    }
}
