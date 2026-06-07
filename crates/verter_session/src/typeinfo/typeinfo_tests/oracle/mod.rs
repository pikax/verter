//! TS7 `TypeExpr`-projection oracle harness (consumption + comparison side).
//!
//! This module is the in-repo, tsgo-free half of the oracle harness defined in
//! `docs/arch/u0-oracle-harness-design.md`: the normalization + canonical
//! comparison engine that lifted `TypeExpr`-projection rows will use to assert
//! parity against checked-in TS7 snapshots. It lifts ZERO rows on its own — it
//! is the foundation the per-block row-lifts ride on.
//!
//! The two-sided positive-allowlist admission gate's PREDICATE (`admission`) is
//! pure, tsgo-free logic — it walks parsed OXC type ASTs and synthetic
//! `RawSourceSurface` records, so it lives in-tree and is exercised by the
//! discriminating guards now. The GENERATION side that DRIVES it (the
//! `#[cfg(feature = "oracle-gen")]` tsgo LSP driver, the vendored env corpus,
//! the probe synthesizer, the live-resolver `resolve_source_declarations`
//! navigation, and the parse-time `RawSourceSurface` capture) is a separate,
//! feature-gated concern that never enters the default resolver build or the
//! default test gate, preserving the `tsgo`-forbidden-at-runtime invariant.

pub(crate) mod admission;
pub(crate) mod driver;
pub(crate) mod identity;
pub(crate) mod normalize;
pub(crate) mod snapshot;

#[allow(unused_imports)]
pub(crate) use driver::run_row;
