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
//! discriminating guards now. The live-resolver `resolve_source_declarations`
//! navigation (`source_walk`) and the parse-time `RawSourceSurface` capture
//! (design item G, in `verter_compiler`) now BIND the gate to real declarations
//! through the one shared resolver — they add no tsgo and no query-time
//! resolution path (the whole `typeinfo_tests` tree is `#[cfg(test)]`). The
//! GENERATION side that DRIVES the hover answers (the `#[cfg(feature =
//! "oracle-gen")]` tsgo LSP driver, the vendored env corpus, the probe
//! synthesizer) is a separate, feature-gated concern that never enters the
//! default resolver build or the default test gate, preserving the
//! `tsgo`-forbidden-at-runtime invariant.

pub(crate) mod admission;
pub(crate) mod driver;
pub(crate) mod hover_extract;
pub(crate) mod identity;
pub(crate) mod normalize;
pub(crate) mod probe;
pub(crate) mod snapshot;
pub(crate) mod source_walk;

#[allow(unused_imports)]
pub(crate) use driver::run_row;
