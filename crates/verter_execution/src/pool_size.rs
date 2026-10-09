//! Worker-count policy for host-owned execution pools.

/// How many worker threads a host-owned pool resolves to.
///
/// The size resolves once, eagerly at host construction, in BOTH spawn modes:
/// [`resolve`](Self::resolve) (the `available_parallelism()` call) runs up
/// front and the resolved count is handed to the pool regardless of
/// the pool's spawn mode. Under lazy spawning that count is passed to
/// the lazy pool constructor; only the OS-thread spawn itself is deferred —
/// never the size resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolSize {
    /// [`std::thread::available_parallelism`] (final-fallback `1` when the
    /// platform cannot report it).
    AvailableParallelism,
    /// Exactly `n` workers (`0` is clamped up to `1`; the pool always has
    /// at least one worker).
    Fixed(usize),
    /// `(available_parallelism / divisor)` clamped to the `{ min, max }`
    /// bounds. The bounds are ORDERED before clamping (an inverted `min > max`
    /// is tolerated) and `divisor == 0` is floored to `1`, so every public
    /// value resolves without panicking. The decl-lowering default is
    /// `Fraction { divisor: 4, min: 1, max: 4 }`. When the platform cannot
    /// report parallelism the fallback is `2`, likewise clamped to the ordered
    /// bounds (matching the historical decl-lowering sizing).
    Fraction {
        divisor: usize,
        min: usize,
        max: usize,
    },
}

impl PoolSize {
    /// Resolve this size to a concrete worker count (always `>= 1`).
    ///
    /// TOTAL over all public inputs: a malformed [`PoolSize::Fraction`]
    /// (`divisor == 0`, or `min > max`) never panics. The divisor is floored
    /// to `1` (no divide-by-zero) and the `{ min, max }` bounds are ORDERED
    /// before clamping, so inverted bounds resolve to a sane in-range value
    /// instead of tripping `clamp`'s `min <= max` requirement. BOTH the
    /// computed value AND the `available_parallelism` fallback are clamped to
    /// the caller's ordered bounds and floored at `1`.
    pub fn resolve(self) -> usize {
        match self {
            PoolSize::AvailableParallelism => std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1),
            PoolSize::Fixed(n) => n.max(1),
            PoolSize::Fraction { divisor, min, max } => {
                // Order the bounds so an inverted `{ min, max }` cannot trip
                // `clamp` (which requires `min <= max`), and floor the divisor
                // so `divisor == 0` cannot divide-by-zero.
                let lo = min.min(max);
                let hi = min.max(max);
                let divisor = divisor.max(1);
                std::thread::available_parallelism()
                    .map(|n| n.get() / divisor)
                    .unwrap_or(2)
                    .clamp(lo, hi)
                    .max(1)
            }
        }
    }
}
