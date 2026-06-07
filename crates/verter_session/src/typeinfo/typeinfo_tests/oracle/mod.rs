//! TS7 `TypeExpr`-projection oracle harness (consumption + comparison side).
//!
//! This module is the in-repo, tsgo-free half of the oracle harness defined in
//! `docs/arch/u0-oracle-harness-design.md`: the normalization + canonical
//! comparison engine that lifted `TypeExpr`-projection rows will use to assert
//! parity against checked-in TS7 snapshots. It lifts ZERO rows on its own — it
//! is the foundation the per-block row-lifts ride on.
//!
//! The generation side (the `#[cfg(feature = "oracle-gen")]` tsgo LSP driver,
//! the vendored env corpus, the probe synthesizer, and the two-sided
//! positive-allowlist admission gate) is a separate, feature-gated concern that
//! never enters the default resolver build or the default test gate, preserving
//! the `tsgo`-forbidden-at-runtime invariant.

pub(crate) mod normalize;
