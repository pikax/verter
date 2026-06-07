//! TS7 `TypeExpr`-projection oracle harness (consumption + comparison side).
//!
//! This module is the in-repo, tsgo-free half of the oracle harness defined in
//! `docs/arch/u0-oracle-harness-design.md`: the normalization + canonical
//! comparison engine that lifted `TypeExpr`-projection rows will use to assert
//! parity against checked-in TS7 snapshots. It lifts ZERO rows on its own — it
//! is the foundation that later row-lifts ride on.
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
pub(crate) mod hover_extract;
pub(crate) mod identity;
pub(crate) mod normalize;
pub(crate) mod probe;
pub(crate) mod snapshot;
pub(crate) mod source_walk;

// The pure-data oracle-query-spec registry. It lives physically at the
// design-pinned path `typeinfo_tests/oracle_query_specs.rs`
// (`registry_in_src_carries_oracle_family`, and the `tests/` guards `include!`
// it from there), but it is PURE context-neutral data (closed enums + owned
// `&'static str`, no `use super`), so it compiles here as `oracle_core::query_specs`
// via `#[path]` — reachable in non-test `oracle-gen` mode by the generator. The
// `#[cfg(test)]` tree reaches the SAME table through the `oracle::query_specs`
// alias, so there is exactly one in-crate compilation of it.
#[path = "../typeinfo_tests/oracle_query_specs.rs"]
pub(crate) mod query_specs;

// The consumption-side shared registry driver + helper dispatch — it builds a
// `VerterHost`, runs the `support.rs` test helpers, and compares Verter's
// in-process `TypeExpr` against the checked-in snapshot. It depends on the
// `#[cfg(test)]`-only `typeinfo_tests::support` helpers, so it is itself
// test-only; the `oracle-gen` generator never consults the resolver (it drives
// tsgo), so the generator build does not compile it.
#[cfg(test)]
pub(crate) mod driver;

// The tsgo-driving snapshot GENERATOR — behind `oracle-gen` only, so the default
// resolver build + default test gate stay tsgo-free (design §3 inv 1). It is the
// build/test-time tool that produces the checked-in snapshots; it is NEVER on the
// consumption path. The `src/bin/oracle_gen` binary invokes its `run_oracle_gen`.
#[cfg(feature = "oracle-gen")]
pub(crate) mod gen;

#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use driver::run_row;

#[cfg(feature = "oracle-gen")]
#[allow(unused_imports)]
pub(crate) use gen::run_oracle_gen;
