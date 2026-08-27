//! Resolution compatibility cases for the production workspace driver and semantic kernel.
//!
//! Full-driver cases execute the workspace production drivers and cross-check the
//! semantic kernel attempt. Pure kernel cases execute `ModuleResolverCore` directly.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex as StdMutex};

use parking_lot::Mutex;
use verter_semantic::resolver_core::{
    AttemptFailure, AttemptOutcome, AttemptOutput, CompletedAttempt,
    ConsumedResolutionObservationKey, IdeProjectConfig, InputKey, ModuleResolverCore, PathProbe,
    ProjectOwnership, ResolutionBasis, ResolutionContext, ResolutionObservationSnapshot,
    ResolutionPackageManifest, ResolutionWorldBasis, ResolvePhase, ResolveRequest,
    ResolveRequestKind, ResolveResult, ResolverAttemptView,
};

pub(crate) const CONVERTED_CASE_COUNT: usize = 24;

/// One row per compatibility case: `name\tkind\treference\tcurrent`.
/// `reference` is the complete fixture for the row: a normalized primitive
/// witness, complete `ResolveResult`, or preferred-specifier list. `current`
/// is `=` when the production driver must reproduce it exactly, or the full
/// expected value where registered carrier extensions add probes beyond the
/// narrow `.vue` reference slice.
///
/// The table is TOTAL: a compatibility case that reaches its assertion without a
/// row panics. Nothing here may decline to fire.
const HISTORICAL_OUTCOMES: &str = r#"full_driver_a_dangling_project_reference_falls_through_without_panicking	witness	{PathProbe { canonical: "/proj/node_modules/unresolvable-specifier.cjs", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier.cts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier.d.cts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier.d.mts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier.js", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier.jsx", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier.mjs", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier.mts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier.tsx", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier.vue", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier/index.cjs", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier/index.cts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier/index.d.cts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier/index.d.mts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier/index.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier/index.js", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier/index.jsx", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier/index.mjs", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier/index.mts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier/index.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier/index.tsx", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier/index.vue", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier.cjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier.d.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier.d.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier.js", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier.jsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier.mjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier.tsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier.vue", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier/index.cjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier/index.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier/index.d.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier/index.d.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier/index.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier/index.js", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier/index.jsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier/index.mjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier/index.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier/index.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier/index.tsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier/index.vue", outcome: Absent }, RecoveryScope { canonical_prefix: "/" }, RecoveryScope { canonical_prefix: "/proj" }, RecoveryScope { canonical_prefix: "/proj/node_modules" }, RecoveryScope { canonical_prefix: "/proj/node_modules/unresolvable-specifier" }, RecoveryScope { canonical_prefix: "/proj/src" }, RecoveryScope { canonical_prefix: "/proj/src/node_modules" }, RecoveryScope { canonical_prefix: "/proj/src/node_modules/unresolvable-specifier" }}	{PathProbe { canonical: "/proj/node_modules/unresolvable-specifier.cjs", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier.cts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier.d.cts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier.d.mts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier.js", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier.jsx", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier.mjs", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier.mts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier.svelte", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier.tsx", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier.vue", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier/index.cjs", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier/index.cts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier/index.d.cts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier/index.d.mts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier/index.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier/index.js", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier/index.jsx", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier/index.mjs", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier/index.mts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier/index.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier/index.tsx", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/unresolvable-specifier/index.vue", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier.cjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier.d.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier.d.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier.js", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier.jsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier.mjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier.svelte", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier.tsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier.vue", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier/index.cjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier/index.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier/index.d.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier/index.d.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier/index.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier/index.js", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier/index.jsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier/index.mjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier/index.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier/index.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier/index.tsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/unresolvable-specifier/index.vue", outcome: Absent }, RecoveryScope { canonical_prefix: "/" }, RecoveryScope { canonical_prefix: "/proj" }, RecoveryScope { canonical_prefix: "/proj/node_modules" }, RecoveryScope { canonical_prefix: "/proj/node_modules/unresolvable-specifier" }, RecoveryScope { canonical_prefix: "/proj/src" }, RecoveryScope { canonical_prefix: "/proj/src/node_modules" }, RecoveryScope { canonical_prefix: "/proj/src/node_modules/unresolvable-specifier" }}
full_driver_a_full_chain_miss_agrees_on_both_engines	witness	{PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz.cjs", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz.cts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz.d.cts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz.d.mts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz.js", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz.jsx", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz.mjs", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz.mts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz.tsx", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz.vue", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz/index.cjs", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz/index.cts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz/index.d.cts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz/index.d.mts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz/index.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz/index.js", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz/index.jsx", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz/index.mjs", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz/index.mts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz/index.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz/index.tsx", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz/index.vue", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz.cjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz.d.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz.d.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz.js", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz.jsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz.mjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz.tsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz.vue", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz/index.cjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz/index.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz/index.d.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz/index.d.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz/index.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz/index.js", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz/index.jsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz/index.mjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz/index.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz/index.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz/index.tsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz/index.vue", outcome: Absent }, RecoveryScope { canonical_prefix: "/" }, RecoveryScope { canonical_prefix: "/proj" }, RecoveryScope { canonical_prefix: "/proj/node_modules" }, RecoveryScope { canonical_prefix: "/proj/node_modules/totally-unresolvable-xyz" }, RecoveryScope { canonical_prefix: "/proj/src" }, RecoveryScope { canonical_prefix: "/proj/src/node_modules" }, RecoveryScope { canonical_prefix: "/proj/src/node_modules/totally-unresolvable-xyz" }}	{PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz.cjs", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz.cts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz.d.cts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz.d.mts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz.js", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz.jsx", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz.mjs", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz.mts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz.svelte", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz.tsx", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz.vue", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz/index.cjs", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz/index.cts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz/index.d.cts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz/index.d.mts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz/index.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz/index.js", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz/index.jsx", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz/index.mjs", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz/index.mts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz/index.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz/index.tsx", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/totally-unresolvable-xyz/index.vue", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz.cjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz.d.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz.d.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz.js", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz.jsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz.mjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz.svelte", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz.tsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz.vue", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz/index.cjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz/index.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz/index.d.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz/index.d.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz/index.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz/index.js", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz/index.jsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz/index.mjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz/index.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz/index.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz/index.tsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/totally-unresolvable-xyz/index.vue", outcome: Absent }, RecoveryScope { canonical_prefix: "/" }, RecoveryScope { canonical_prefix: "/proj" }, RecoveryScope { canonical_prefix: "/proj/node_modules" }, RecoveryScope { canonical_prefix: "/proj/node_modules/totally-unresolvable-xyz" }, RecoveryScope { canonical_prefix: "/proj/src" }, RecoveryScope { canonical_prefix: "/proj/src/node_modules" }, RecoveryScope { canonical_prefix: "/proj/src/node_modules/totally-unresolvable-xyz" }}
full_driver_a_project_reference_cycle_terminates_on_both_engines	witness	{PathProbe { canonical: "/proj/c/src/thing.ts", outcome: File }, Realpath { requested: "/proj/c/src/thing.ts", resolved: Some("/proj/c/src/thing.ts") }, RecoveryScope { canonical_prefix: "/" }, RecoveryScope { canonical_prefix: "/proj" }, RecoveryScope { canonical_prefix: "/proj/c" }, RecoveryScope { canonical_prefix: "/proj/c/src" }, RecoveryScope { canonical_prefix: "/proj/c/src/thing" }}	=
full_driver_carrier_import_provider_projection_matches_legacy_end_to_end	dto	ResolveResult { source_id: "/proj/src/Comp.vue", provider_id: "/proj/src/Comp.vue.verter.ts", provider_specifier: "./Comp.vue.verter.ts", provider_target: CarrierPublicApi, resolution_kind: Relative, owner_tsconfig_path: Some("/proj/tsconfig.json") }	=
full_driver_carrier_import_provider_projection_matches_legacy_end_to_end	witness	{PathProbe { canonical: "/proj/src/Comp.vue", outcome: File }, Realpath { requested: "/proj/src/Comp.vue", resolved: Some("/proj/src/Comp.vue") }, RecoveryScope { canonical_prefix: "/" }, RecoveryScope { canonical_prefix: "/proj" }, RecoveryScope { canonical_prefix: "/proj/src" }}	=
full_driver_owner_overlap_selects_the_nearest_root	witness	{PathProbe { canonical: "/proj/pkg/INNER/thing.ts", outcome: File }, Realpath { requested: "/proj/pkg/INNER/thing.ts", resolved: Some("/proj/pkg/INNER/thing.ts") }, RecoveryScope { canonical_prefix: "/" }, RecoveryScope { canonical_prefix: "/proj" }, RecoveryScope { canonical_prefix: "/proj/pkg" }, RecoveryScope { canonical_prefix: "/proj/pkg/INNER" }, RecoveryScope { canonical_prefix: "/proj/pkg/INNER/thing" }}	=
full_driver_preferred_specifier_candidates_agrees_with_legacy	candidates	Some(["@app/thing.ts", "@/app/thing.ts"])	=
full_driver_project_exact_result_agrees_with_legacy	dto	ResolveResult { source_id: "/proj/src/exact.ts", provider_id: "/proj/src/exact.ts", provider_specifier: "whatever", provider_target: ShadowSourceFile, resolution_kind: Bundler, owner_tsconfig_path: Some("/proj/tsconfig.json") }	=
full_driver_resolves_a_relative_specifier_for_an_owned_importer	witness	{PathProbe { canonical: "/proj/src/sibling.ts", outcome: File }, Realpath { requested: "/proj/src/sibling.ts", resolved: Some("/proj/src/sibling.ts") }, RecoveryScope { canonical_prefix: "/" }, RecoveryScope { canonical_prefix: "/proj" }, RecoveryScope { canonical_prefix: "/proj/src" }}	=
full_driver_resolves_a_scoped_package_via_legacy_main_field	witness	{PathProbe { canonical: "/proj/node_modules/@scope/pkg/index.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/@scope/pkg/index.js", outcome: File }, PathProbe { canonical: "/proj/node_modules/@scope/pkg/index.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/@scope/pkg/index.tsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg.cjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg.d.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg.d.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg.js", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg.jsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg.mjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg.tsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg.vue", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg/index.cjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg/index.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg/index.d.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg/index.d.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg/index.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg/index.js", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg/index.jsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg/index.mjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg/index.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg/index.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg/index.tsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg/index.vue", outcome: Absent }, Realpath { requested: "/proj/node_modules/@scope/pkg/index.js", resolved: Some("/proj/node_modules/@scope/pkg/index.js") }, RecoveryScope { canonical_prefix: "/" }, RecoveryScope { canonical_prefix: "/proj" }, RecoveryScope { canonical_prefix: "/proj/node_modules" }, RecoveryScope { canonical_prefix: "/proj/node_modules/@scope" }, RecoveryScope { canonical_prefix: "/proj/node_modules/@scope/pkg" }, RecoveryScope { canonical_prefix: "/proj/src" }, RecoveryScope { canonical_prefix: "/proj/src/node_modules" }, RecoveryScope { canonical_prefix: "/proj/src/node_modules/@scope" }, RecoveryScope { canonical_prefix: "/proj/src/node_modules/@scope/pkg" }}	{PathProbe { canonical: "/proj/node_modules/@scope/pkg/index.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/@scope/pkg/index.js", outcome: File }, PathProbe { canonical: "/proj/node_modules/@scope/pkg/index.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/@scope/pkg/index.tsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg.cjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg.d.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg.d.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg.js", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg.jsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg.mjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg.svelte", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg.tsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg.vue", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg/index.cjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg/index.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg/index.d.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg/index.d.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg/index.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg/index.js", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg/index.jsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg/index.mjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg/index.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg/index.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg/index.tsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/@scope/pkg/index.vue", outcome: Absent }, Realpath { requested: "/proj/node_modules/@scope/pkg/index.js", resolved: Some("/proj/node_modules/@scope/pkg/index.js") }, RecoveryScope { canonical_prefix: "/" }, RecoveryScope { canonical_prefix: "/proj" }, RecoveryScope { canonical_prefix: "/proj/node_modules" }, RecoveryScope { canonical_prefix: "/proj/node_modules/@scope" }, RecoveryScope { canonical_prefix: "/proj/node_modules/@scope/pkg" }, RecoveryScope { canonical_prefix: "/proj/src" }, RecoveryScope { canonical_prefix: "/proj/src/node_modules" }, RecoveryScope { canonical_prefix: "/proj/src/node_modules/@scope" }, RecoveryScope { canonical_prefix: "/proj/src/node_modules/@scope/pkg" }}
full_driver_resolves_an_absolute_specifier_for_an_owned_importer	witness	{PathProbe { canonical: "/abs/target.ts", outcome: File }, Realpath { requested: "/abs/target.ts", resolved: Some("/abs/target.ts") }, RecoveryScope { canonical_prefix: "/" }, RecoveryScope { canonical_prefix: "/abs" }}	=
full_driver_resolves_via_a_project_reference	witness	{PathProbe { canonical: "/proj/b/src/thing.ts", outcome: File }, Realpath { requested: "/proj/b/src/thing.ts", resolved: Some("/proj/b/src/thing.ts") }, RecoveryScope { canonical_prefix: "/" }, RecoveryScope { canonical_prefix: "/proj" }, RecoveryScope { canonical_prefix: "/proj/b" }, RecoveryScope { canonical_prefix: "/proj/b/src" }, RecoveryScope { canonical_prefix: "/proj/b/src/thing" }}	=
full_driver_resolves_via_a_workspace_alias	witness	{PathProbe { canonical: "/proj/src/util.ts", outcome: File }, Realpath { requested: "/proj/src/util.ts", resolved: Some("/proj/src/util.ts") }, RecoveryScope { canonical_prefix: "/" }, RecoveryScope { canonical_prefix: "/proj" }, RecoveryScope { canonical_prefix: "/proj/src" }, RecoveryScope { canonical_prefix: "/proj/src/util" }}	=
full_driver_resolves_via_explicit_project_ownership	witness	{PathProbe { canonical: "/proj/src/thing.ts", outcome: File }, Realpath { requested: "/proj/src/thing.ts", resolved: Some("/proj/src/thing.ts") }, RecoveryScope { canonical_prefix: "/" }, RecoveryScope { canonical_prefix: "/proj" }, RecoveryScope { canonical_prefix: "/proj/src" }, RecoveryScope { canonical_prefix: "/proj/src/thing" }}	=
full_driver_resolves_via_hash_imports	witness	{PathProbe { canonical: "/proj/src/utils/format.ts", outcome: File }, Realpath { requested: "/proj/src/utils/format.ts", resolved: Some("/proj/src/utils/format.ts") }, RecoveryScope { canonical_prefix: "/" }, RecoveryScope { canonical_prefix: "/proj" }, RecoveryScope { canonical_prefix: "/proj/src" }, RecoveryScope { canonical_prefix: "/proj/src/utils" }}	=
full_driver_resolves_via_node_modules_exports_array_form	witness	{PathProbe { canonical: "/proj/node_modules/pkg/dist/first.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/first.js", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/first.js/index.cjs", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/first.js/index.cts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/first.js/index.d.cts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/first.js/index.d.mts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/first.js/index.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/first.js/index.js", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/first.js/index.jsx", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/first.js/index.mjs", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/first.js/index.mts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/first.js/index.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/first.js/index.tsx", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/first.js/index.vue", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/first.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/first.tsx", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/second.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/second.js", outcome: File }, PathProbe { canonical: "/proj/node_modules/pkg/dist/second.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/second.tsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg.cjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg.d.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg.d.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg.js", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg.jsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg.mjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg.tsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg.vue", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg/index.cjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg/index.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg/index.d.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg/index.d.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg/index.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg/index.js", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg/index.jsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg/index.mjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg/index.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg/index.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg/index.tsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg/index.vue", outcome: Absent }, Realpath { requested: "/proj/node_modules/pkg/dist/second.js", resolved: Some("/proj/node_modules/pkg/dist/second.js") }, RecoveryScope { canonical_prefix: "/" }, RecoveryScope { canonical_prefix: "/proj" }, RecoveryScope { canonical_prefix: "/proj/node_modules" }, RecoveryScope { canonical_prefix: "/proj/node_modules/pkg" }, RecoveryScope { canonical_prefix: "/proj/node_modules/pkg/dist" }, RecoveryScope { canonical_prefix: "/proj/node_modules/pkg/dist/first.js" }, RecoveryScope { canonical_prefix: "/proj/src" }, RecoveryScope { canonical_prefix: "/proj/src/node_modules" }, RecoveryScope { canonical_prefix: "/proj/src/node_modules/pkg" }}	{PathProbe { canonical: "/proj/node_modules/pkg/dist/first.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/first.js", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/first.js/index.cjs", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/first.js/index.cts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/first.js/index.d.cts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/first.js/index.d.mts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/first.js/index.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/first.js/index.js", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/first.js/index.jsx", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/first.js/index.mjs", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/first.js/index.mts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/first.js/index.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/first.js/index.tsx", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/first.js/index.vue", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/first.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/first.tsx", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/second.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/second.js", outcome: File }, PathProbe { canonical: "/proj/node_modules/pkg/dist/second.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/pkg/dist/second.tsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg.cjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg.d.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg.d.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg.js", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg.jsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg.mjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg.svelte", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg.tsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg.vue", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg/index.cjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg/index.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg/index.d.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg/index.d.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg/index.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg/index.js", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg/index.jsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg/index.mjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg/index.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg/index.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg/index.tsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/pkg/index.vue", outcome: Absent }, Realpath { requested: "/proj/node_modules/pkg/dist/second.js", resolved: Some("/proj/node_modules/pkg/dist/second.js") }, RecoveryScope { canonical_prefix: "/" }, RecoveryScope { canonical_prefix: "/proj" }, RecoveryScope { canonical_prefix: "/proj/node_modules" }, RecoveryScope { canonical_prefix: "/proj/node_modules/pkg" }, RecoveryScope { canonical_prefix: "/proj/node_modules/pkg/dist" }, RecoveryScope { canonical_prefix: "/proj/node_modules/pkg/dist/first.js" }, RecoveryScope { canonical_prefix: "/proj/src" }, RecoveryScope { canonical_prefix: "/proj/src/node_modules" }, RecoveryScope { canonical_prefix: "/proj/src/node_modules/pkg" }}
full_driver_resolves_via_node_modules_exports_with_conditions	witness	{PathProbe { canonical: "/proj/node_modules/lodash/esm/index.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/lodash/esm/index.js", outcome: File }, PathProbe { canonical: "/proj/node_modules/lodash/esm/index.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/lodash/esm/index.tsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash.cjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash.d.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash.d.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash.js", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash.jsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash.mjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash.tsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash.vue", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash/index.cjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash/index.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash/index.d.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash/index.d.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash/index.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash/index.js", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash/index.jsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash/index.mjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash/index.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash/index.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash/index.tsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash/index.vue", outcome: Absent }, Realpath { requested: "/proj/node_modules/lodash/esm/index.js", resolved: Some("/proj/node_modules/lodash/esm/index.js") }, RecoveryScope { canonical_prefix: "/" }, RecoveryScope { canonical_prefix: "/proj" }, RecoveryScope { canonical_prefix: "/proj/node_modules" }, RecoveryScope { canonical_prefix: "/proj/node_modules/lodash" }, RecoveryScope { canonical_prefix: "/proj/node_modules/lodash/esm" }, RecoveryScope { canonical_prefix: "/proj/src" }, RecoveryScope { canonical_prefix: "/proj/src/node_modules" }, RecoveryScope { canonical_prefix: "/proj/src/node_modules/lodash" }}	{PathProbe { canonical: "/proj/node_modules/lodash/esm/index.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/lodash/esm/index.js", outcome: File }, PathProbe { canonical: "/proj/node_modules/lodash/esm/index.ts", outcome: Absent }, PathProbe { canonical: "/proj/node_modules/lodash/esm/index.tsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash.cjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash.d.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash.d.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash.js", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash.jsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash.mjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash.svelte", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash.tsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash.vue", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash/index.cjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash/index.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash/index.d.cts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash/index.d.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash/index.d.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash/index.js", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash/index.jsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash/index.mjs", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash/index.mts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash/index.ts", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash/index.tsx", outcome: Absent }, PathProbe { canonical: "/proj/src/node_modules/lodash/index.vue", outcome: Absent }, Realpath { requested: "/proj/node_modules/lodash/esm/index.js", resolved: Some("/proj/node_modules/lodash/esm/index.js") }, RecoveryScope { canonical_prefix: "/" }, RecoveryScope { canonical_prefix: "/proj" }, RecoveryScope { canonical_prefix: "/proj/node_modules" }, RecoveryScope { canonical_prefix: "/proj/node_modules/lodash" }, RecoveryScope { canonical_prefix: "/proj/node_modules/lodash/esm" }, RecoveryScope { canonical_prefix: "/proj/src" }, RecoveryScope { canonical_prefix: "/proj/src/node_modules" }, RecoveryScope { canonical_prefix: "/proj/src/node_modules/lodash" }}
full_driver_resolves_via_the_base_url_fallback	witness	{PathProbe { canonical: "/proj/src2/thing.ts", outcome: File }, Realpath { requested: "/proj/src2/thing.ts", resolved: Some("/proj/src2/thing.ts") }, RecoveryScope { canonical_prefix: "/" }, RecoveryScope { canonical_prefix: "/proj" }, RecoveryScope { canonical_prefix: "/proj/src2" }, RecoveryScope { canonical_prefix: "/proj/src2/thing" }}	=
full_driver_resolves_via_tsconfig_paths	witness	{PathProbe { canonical: "/proj/src/app/thing.ts", outcome: File }, Realpath { requested: "/proj/src/app/thing.ts", resolved: Some("/proj/src/app/thing.ts") }, RecoveryScope { canonical_prefix: "/" }, RecoveryScope { canonical_prefix: "/proj" }, RecoveryScope { canonical_prefix: "/proj/src" }, RecoveryScope { canonical_prefix: "/proj/src/app" }, RecoveryScope { canonical_prefix: "/proj/src/app/thing" }}	=
full_driver_workspace_alias_wins_over_tsconfig_paths_and_base_url	witness	{PathProbe { canonical: "/proj/alias/thing.ts", outcome: File }, Realpath { requested: "/proj/alias/thing.ts", resolved: Some("/proj/alias/thing.ts") }, RecoveryScope { canonical_prefix: "/" }, RecoveryScope { canonical_prefix: "/proj" }, RecoveryScope { canonical_prefix: "/proj/alias" }, RecoveryScope { canonical_prefix: "/proj/alias/thing" }}	=
kernel_runner_miss_case_matches_the_witness_contract_result	witness	{PathProbe { canonical: "/p/missing.cjs", outcome: Absent }, PathProbe { canonical: "/p/missing.cts", outcome: Absent }, PathProbe { canonical: "/p/missing.d.cts", outcome: Absent }, PathProbe { canonical: "/p/missing.d.mts", outcome: Absent }, PathProbe { canonical: "/p/missing.d.ts", outcome: Absent }, PathProbe { canonical: "/p/missing.js", outcome: Absent }, PathProbe { canonical: "/p/missing.jsx", outcome: Absent }, PathProbe { canonical: "/p/missing.mjs", outcome: Absent }, PathProbe { canonical: "/p/missing.mts", outcome: Absent }, PathProbe { canonical: "/p/missing.ts", outcome: Absent }, PathProbe { canonical: "/p/missing.tsx", outcome: Absent }, PathProbe { canonical: "/p/missing.vue", outcome: Absent }, PathProbe { canonical: "/p/missing/index.cjs", outcome: Absent }, PathProbe { canonical: "/p/missing/index.cts", outcome: Absent }, PathProbe { canonical: "/p/missing/index.d.cts", outcome: Absent }, PathProbe { canonical: "/p/missing/index.d.mts", outcome: Absent }, PathProbe { canonical: "/p/missing/index.d.ts", outcome: Absent }, PathProbe { canonical: "/p/missing/index.js", outcome: Absent }, PathProbe { canonical: "/p/missing/index.jsx", outcome: Absent }, PathProbe { canonical: "/p/missing/index.mjs", outcome: Absent }, PathProbe { canonical: "/p/missing/index.mts", outcome: Absent }, PathProbe { canonical: "/p/missing/index.ts", outcome: Absent }, PathProbe { canonical: "/p/missing/index.tsx", outcome: Absent }, PathProbe { canonical: "/p/missing/index.vue", outcome: Absent }, RecoveryScope { canonical_prefix: "/" }, RecoveryScope { canonical_prefix: "/p" }, RecoveryScope { canonical_prefix: "/p/missing" }}	{PathProbe { canonical: "/p/missing.cjs", outcome: Absent }, PathProbe { canonical: "/p/missing.cts", outcome: Absent }, PathProbe { canonical: "/p/missing.d.cts", outcome: Absent }, PathProbe { canonical: "/p/missing.d.mts", outcome: Absent }, PathProbe { canonical: "/p/missing.d.ts", outcome: Absent }, PathProbe { canonical: "/p/missing.js", outcome: Absent }, PathProbe { canonical: "/p/missing.jsx", outcome: Absent }, PathProbe { canonical: "/p/missing.mjs", outcome: Absent }, PathProbe { canonical: "/p/missing.mts", outcome: Absent }, PathProbe { canonical: "/p/missing.svelte", outcome: Absent }, PathProbe { canonical: "/p/missing.ts", outcome: Absent }, PathProbe { canonical: "/p/missing.tsx", outcome: Absent }, PathProbe { canonical: "/p/missing.vue", outcome: Absent }, PathProbe { canonical: "/p/missing/index.cjs", outcome: Absent }, PathProbe { canonical: "/p/missing/index.cts", outcome: Absent }, PathProbe { canonical: "/p/missing/index.d.cts", outcome: Absent }, PathProbe { canonical: "/p/missing/index.d.mts", outcome: Absent }, PathProbe { canonical: "/p/missing/index.d.ts", outcome: Absent }, PathProbe { canonical: "/p/missing/index.js", outcome: Absent }, PathProbe { canonical: "/p/missing/index.jsx", outcome: Absent }, PathProbe { canonical: "/p/missing/index.mjs", outcome: Absent }, PathProbe { canonical: "/p/missing/index.mts", outcome: Absent }, PathProbe { canonical: "/p/missing/index.ts", outcome: Absent }, PathProbe { canonical: "/p/missing/index.tsx", outcome: Absent }, PathProbe { canonical: "/p/missing/index.vue", outcome: Absent }, RecoveryScope { canonical_prefix: "/" }, RecoveryScope { canonical_prefix: "/p" }, RecoveryScope { canonical_prefix: "/p/missing" }}
kernel_runner_positive_case_matches_the_witness_contract_result	witness	{PathProbe { canonical: "/p/mod.ts", outcome: Absent }, PathProbe { canonical: "/p/mod.tsx", outcome: File }, Realpath { requested: "/p/mod.tsx", resolved: Some("/store/pkg/mod.tsx") }, RecoveryScope { canonical_prefix: "/" }, RecoveryScope { canonical_prefix: "/p" }, RecoveryScope { canonical_prefix: "/store" }, RecoveryScope { canonical_prefix: "/store/pkg" }}	=
kernel_runner_restarts_cleanly_on_a_basis_change	witness	{PathProbe { canonical: "/p/mod.ts", outcome: Absent }, PathProbe { canonical: "/p/mod.tsx", outcome: File }, Realpath { requested: "/p/mod.tsx", resolved: Some("/store/pkg/mod.tsx") }, RecoveryScope { canonical_prefix: "/" }, RecoveryScope { canonical_prefix: "/p" }, RecoveryScope { canonical_prefix: "/store" }, RecoveryScope { canonical_prefix: "/store/pkg" }}	=
resolution_witness_miss_case_kernel_matches_legacy	witness	{PathProbe { canonical: "/p/missing.cjs", outcome: Absent }, PathProbe { canonical: "/p/missing.cts", outcome: Absent }, PathProbe { canonical: "/p/missing.d.cts", outcome: Absent }, PathProbe { canonical: "/p/missing.d.mts", outcome: Absent }, PathProbe { canonical: "/p/missing.d.ts", outcome: Absent }, PathProbe { canonical: "/p/missing.js", outcome: Absent }, PathProbe { canonical: "/p/missing.jsx", outcome: Absent }, PathProbe { canonical: "/p/missing.mjs", outcome: Absent }, PathProbe { canonical: "/p/missing.mts", outcome: Absent }, PathProbe { canonical: "/p/missing.ts", outcome: Absent }, PathProbe { canonical: "/p/missing.tsx", outcome: Absent }, PathProbe { canonical: "/p/missing.vue", outcome: Absent }, PathProbe { canonical: "/p/missing/index.cjs", outcome: Absent }, PathProbe { canonical: "/p/missing/index.cts", outcome: Absent }, PathProbe { canonical: "/p/missing/index.d.cts", outcome: Absent }, PathProbe { canonical: "/p/missing/index.d.mts", outcome: Absent }, PathProbe { canonical: "/p/missing/index.d.ts", outcome: Absent }, PathProbe { canonical: "/p/missing/index.js", outcome: Absent }, PathProbe { canonical: "/p/missing/index.jsx", outcome: Absent }, PathProbe { canonical: "/p/missing/index.mjs", outcome: Absent }, PathProbe { canonical: "/p/missing/index.mts", outcome: Absent }, PathProbe { canonical: "/p/missing/index.ts", outcome: Absent }, PathProbe { canonical: "/p/missing/index.tsx", outcome: Absent }, PathProbe { canonical: "/p/missing/index.vue", outcome: Absent }, RecoveryScope { canonical_prefix: "/" }, RecoveryScope { canonical_prefix: "/p" }, RecoveryScope { canonical_prefix: "/p/missing" }}	{PathProbe { canonical: "/p/missing.cjs", outcome: Absent }, PathProbe { canonical: "/p/missing.cts", outcome: Absent }, PathProbe { canonical: "/p/missing.d.cts", outcome: Absent }, PathProbe { canonical: "/p/missing.d.mts", outcome: Absent }, PathProbe { canonical: "/p/missing.d.ts", outcome: Absent }, PathProbe { canonical: "/p/missing.js", outcome: Absent }, PathProbe { canonical: "/p/missing.jsx", outcome: Absent }, PathProbe { canonical: "/p/missing.mjs", outcome: Absent }, PathProbe { canonical: "/p/missing.mts", outcome: Absent }, PathProbe { canonical: "/p/missing.svelte", outcome: Absent }, PathProbe { canonical: "/p/missing.ts", outcome: Absent }, PathProbe { canonical: "/p/missing.tsx", outcome: Absent }, PathProbe { canonical: "/p/missing.vue", outcome: Absent }, PathProbe { canonical: "/p/missing/index.cjs", outcome: Absent }, PathProbe { canonical: "/p/missing/index.cts", outcome: Absent }, PathProbe { canonical: "/p/missing/index.d.cts", outcome: Absent }, PathProbe { canonical: "/p/missing/index.d.mts", outcome: Absent }, PathProbe { canonical: "/p/missing/index.d.ts", outcome: Absent }, PathProbe { canonical: "/p/missing/index.js", outcome: Absent }, PathProbe { canonical: "/p/missing/index.jsx", outcome: Absent }, PathProbe { canonical: "/p/missing/index.mjs", outcome: Absent }, PathProbe { canonical: "/p/missing/index.mts", outcome: Absent }, PathProbe { canonical: "/p/missing/index.ts", outcome: Absent }, PathProbe { canonical: "/p/missing/index.tsx", outcome: Absent }, PathProbe { canonical: "/p/missing/index.vue", outcome: Absent }, RecoveryScope { canonical_prefix: "/" }, RecoveryScope { canonical_prefix: "/p" }, RecoveryScope { canonical_prefix: "/p/missing" }}
resolution_witness_positive_case_kernel_matches_legacy	witness	{PathProbe { canonical: "/p/mod.ts", outcome: Absent }, PathProbe { canonical: "/p/mod.tsx", outcome: File }, Realpath { requested: "/p/mod.tsx", resolved: Some("/store/pkg/mod.tsx") }, RecoveryScope { canonical_prefix: "/" }, RecoveryScope { canonical_prefix: "/p" }, RecoveryScope { canonical_prefix: "/store" }, RecoveryScope { canonical_prefix: "/store/pkg" }}	="#;

#[derive(Debug, Default)]
pub(super) struct ResolutionFixture {
    files: BTreeSet<String>,
    realpaths: BTreeMap<String, String>,
    manifests: BTreeMap<String, ResolutionPackageManifest>,
    probe_directory_observations: BTreeMap<String, Vec<String>>,
}

impl ResolutionFixture {
    pub(super) fn new(files: &[&str]) -> Self {
        Self {
            files: files.iter().map(|path| (*path).to_string()).collect(),
            ..Self::default()
        }
    }

    fn with_realpath(mut self, requested: &str, resolved: &str) -> Self {
        self.realpaths
            .insert(requested.to_string(), resolved.to_string());
        self
    }

    fn with_manifest(mut self, directory: &str, manifest: ResolutionPackageManifest) -> Self {
        self.files.insert(format!("{directory}/package.json"));
        self.manifests.insert(directory.to_string(), manifest);
        self
    }

    pub(super) fn with_probe_directory_observation(mut self, path: &str, directory: &str) -> Self {
        self.probe_directory_observations
            .entry(path.to_string())
            .or_default()
            .push(directory.to_string());
        self
    }

    fn probe(&self, path: &str) -> PathProbe {
        if self.files.contains(path) {
            PathProbe::File
        } else {
            PathProbe::Absent
        }
    }

    fn realpath_of(&self, path: &str) -> Option<String> {
        self.realpaths
            .get(path)
            .cloned()
            .or_else(|| self.files.contains(path).then(|| path.to_string()))
    }
}

struct FixtureReader<'a> {
    fixture: &'a ResolutionFixture,
    pending_directory_observations: StdMutex<Vec<String>>,
}

impl crate::traits::WorkspaceRead for FixtureReader<'_> {
    fn read_file(&self, canonical_id: &str) -> Option<Arc<str>> {
        self.fixture
            .files
            .contains(canonical_id)
            .then(|| Arc::from("// resolution conversion fixture"))
    }

    fn file_exists(&self, canonical_id: &str) -> bool {
        matches!(self.probe_path(canonical_id), PathProbe::File)
    }

    fn probe_path(&self, canonical_id: &str) -> PathProbe {
        if let Some(directories) = self.fixture.probe_directory_observations.get(canonical_id) {
            self.pending_directory_observations
                .lock()
                .expect("fixture directory observations mutex poisoned")
                .extend(directories.iter().cloned());
        }
        self.fixture.probe(canonical_id)
    }

    fn take_resolution_directory_observations(&self) -> Vec<String> {
        std::mem::take(
            &mut *self
                .pending_directory_observations
                .lock()
                .expect("fixture directory observations mutex poisoned"),
        )
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        self.fixture.realpath_of(canonical_id)
    }

    fn read_package_manifest(&self, canonical_id: &str) -> Option<crate::types::PackageManifest> {
        let directory = canonical_id
            .strip_suffix("/package.json")
            .unwrap_or(canonical_id);
        let manifest = self.fixture.manifests.get(directory)?;
        Some(crate::types::PackageManifest {
            name: None,
            version: None,
            main: manifest.main.clone(),
            module: manifest.module.clone(),
            types: manifest.types.clone(),
            typings: manifest.typings.clone(),
            exports: manifest.exports.clone(),
            imports: manifest.imports.clone(),
            raw: None,
        })
    }

    fn preflight_resolution_inputs_bounded(
        &self,
        keys: &[InputKey],
        basis: ResolutionBasis,
    ) -> Result<crate::resolver::ResolutionInputReservationBatch, AttemptFailure> {
        crate::resolver::preflight_supported_resolution_inputs(
            keys,
            basis,
            |path| {
                let value = self.probe_path(path);
                Ok((value, self.take_resolution_directory_observations()))
            },
            |path| Ok((self.realpath(path), Vec::new())),
            |manifest_path, _| {
                let manifest = self.read_package_manifest(manifest_path);
                Ok((manifest.is_some(), 0, Vec::new()))
            },
        )
    }

    fn load_preflighted_resolution_inputs(
        &self,
        reservation: &crate::resolver::ResolutionInputReservationBatch,
    ) -> Result<crate::resolver::LoadedResolutionInputBatch, AttemptFailure> {
        crate::resolver::load_supported_resolution_inputs(
            reservation,
            |manifest_path, expected_present, _, key| {
                let manifest = self.read_package_manifest(manifest_path);
                if manifest.is_some() != expected_present {
                    return Err(AttemptFailure::InputLoadIntegrity {
                        unresolved: vec![key.clone()],
                        reason: verter_semantic::resolver_core::InputLoadIntegrityReason::IncompleteBoundedCapture,
                    });
                }
                Ok(manifest)
            },
        )
    }

    fn reverse_deps_for(&self, _id: &str) -> Vec<String> {
        Vec::new()
    }

    fn forward_deps_for(&self, _id: &str) -> Vec<String> {
        Vec::new()
    }

    fn dependency_snapshot(
        &self,
        _id: &str,
    ) -> Option<crate::exact_resolution::DependencySnapshotView> {
        None
    }
}

#[derive(Debug, Clone, Default)]
struct FullKernelSnapshot {
    observations: Arc<ResolutionObservationSnapshot>,
    path_probes: BTreeMap<String, PathProbe>,
    realpaths: BTreeMap<String, Option<String>>,
    manifests: BTreeMap<String, Option<Arc<ResolutionPackageManifest>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum WitnessFact {
    PathProbe {
        canonical: String,
        outcome: PathProbe,
    },
    Realpath {
        requested: String,
        resolved: Option<String>,
    },
    RecoveryScope {
        canonical_prefix: String,
    },
}

fn ancestor_scopes(path: &str) -> Vec<String> {
    let mut scopes = Vec::new();
    let mut current = path;
    while let Some(index) = current.rfind('/') {
        let prefix = if index == 0 { "/" } else { &current[..index] };
        scopes.push(prefix.to_string());
        if prefix == "/" {
            break;
        }
        current = prefix;
    }
    scopes
}

fn normalized_kernel_witness(
    output: &AttemptOutput,
    snapshot: &FullKernelSnapshot,
) -> BTreeSet<WitnessFact> {
    let mut witness = BTreeSet::new();
    for key in output.consumed_resolution_observations() {
        match key {
            ConsumedResolutionObservationKey::PathProbe { path } => {
                if !path.ends_with("/package.json") {
                    witness.insert(WitnessFact::PathProbe {
                        canonical: path.to_string(),
                        outcome: *snapshot
                            .path_probes
                            .get(path.as_ref())
                            .expect("consumed probe must be loaded"),
                    });
                }
            }
            ConsumedResolutionObservationKey::RealPath { path } => {
                witness.insert(WitnessFact::Realpath {
                    requested: path.to_string(),
                    resolved: snapshot
                        .realpaths
                        .get(path.as_ref())
                        .expect("consumed realpath must be loaded")
                        .clone(),
                });
            }
            ConsumedResolutionObservationKey::RecoveryScope { canonical_prefix } => {
                witness.insert(WitnessFact::RecoveryScope {
                    canonical_prefix: canonical_prefix.to_string(),
                });
            }
            ConsumedResolutionObservationKey::PackageManifest { directory } => {
                // The compatibility fixture normalizes the kernel's single
                // manifest observation against the probe+read boundary by
                // retaining the manifest path's ancestor recovery scopes only.
                for prefix in ancestor_scopes(&format!("{directory}/package.json")) {
                    witness.insert(WitnessFact::RecoveryScope {
                        canonical_prefix: prefix,
                    });
                }
            }
        }
    }
    witness
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutcomeKind {
    Witness,
    Dto,
    Candidates,
}

impl OutcomeKind {
    fn tag(self) -> &'static str {
        match self {
            OutcomeKind::Witness => "witness",
            OutcomeKind::Dto => "dto",
            OutcomeKind::Candidates => "candidates",
        }
    }
}

struct HistoricalOutcome {
    name: &'static str,
    kind: &'static str,
    historical: &'static str,
    current: &'static str,
}

impl HistoricalOutcome {
    fn expected(&self) -> &'static str {
        if self.current == "=" {
            self.historical
        } else {
            self.current
        }
    }
}

fn historical_outcomes() -> Vec<HistoricalOutcome> {
    HISTORICAL_OUTCOMES
        .lines()
        .map(|line| {
            let mut cols = line.split('\t');
            let row = HistoricalOutcome {
                name: cols.next().expect("row name"),
                kind: cols.next().expect("row kind"),
                historical: cols.next().expect("row historical value"),
                current: cols.next().expect("row current value"),
            };
            assert!(
                cols.next().is_none(),
                "row {} has trailing columns",
                row.name
            );
            row
        })
        .collect()
}

fn current_case_name() -> String {
    // A check that can decline to fire and say nothing is worse than no
    // check: whether the harness names its threads is a property of
    // `--test-threads`, not of the code under test, so nobody would think
    // to look here. Fail loudly instead.
    let current_thread = std::thread::current();
    match current_thread
        .name()
        .and_then(|name| name.rsplit("::").next())
    {
        Some(name) => name.to_string(),
        None => panic!(
            "historical-outcome check could not resolve a case name from the current \
             thread (thread name: {:?}); it must never skip silently",
            current_thread.name()
        ),
    }
}

/// Total lookup: the running case MUST have a ledger row of `kind`, and the
/// production outcome MUST equal the row's expected value. A missing row, a
/// row of another kind, or a divergent value all fail — there is no
/// declining arm.
fn assert_historical_outcome(kind: OutcomeKind, actual: &str) {
    let case = current_case_name();
    let rows = historical_outcomes();
    let row = rows
        .iter()
        .find(|row| row.name == case && row.kind == kind.tag())
        .unwrap_or_else(|| {
            let present: Vec<&str> = rows
                .iter()
                .filter(|row| row.name == case)
                .map(|row| row.kind)
                .collect();
            panic!(
                "converted case {case:?} has no HISTORICAL_OUTCOMES row of kind {:?} (rows \
                 present: {present:?}); every converted case must carry its captured \
                 historical outcome for every assertion it makes",
                kind.tag()
            )
        });
    assert_eq!(
        actual,
        row.expected(),
        "converted case {case:?}: the production driver diverged from the retained outcome \
         (historical: {})",
        row.historical
    );
}

fn assert_historical_witness(witness: &BTreeSet<WitnessFact>) {
    assert_historical_outcome(OutcomeKind::Witness, &format!("{witness:?}"));
}

fn kernel_basis() -> ResolutionBasis {
    basis_for(0xC001)
}

fn basis_for(raw: u64) -> ResolutionBasis {
    ResolutionBasis::new(
        ResolutionWorldBasis::new(
            verter_semantic::resolver_core::WorkspaceAuthorityId::from_raw(raw),
            verter_semantic::resolver_core::ResolutionPopulation::Base,
            verter_semantic::resolver_core::ResolutionWorldId::from_raw(raw),
            None,
        ),
        None,
    )
}

fn build_full_attempt_view(
    snapshot: &FullKernelSnapshot,
    missing_basis: ResolutionBasis,
) -> ResolverAttemptView {
    ResolverAttemptView::from_resolution_snapshot(Arc::clone(&snapshot.observations), missing_basis)
}

fn load_full_snapshot(
    snapshot: &mut FullKernelSnapshot,
    fixture: &ResolutionFixture,
    keys: &[InputKey],
) -> bool {
    let mut progressed = false;
    for key in keys {
        match key {
            InputKey::PathProbe { path } => {
                if !snapshot.path_probes.contains_key(path.as_ref()) {
                    let probe = fixture.probe(path);
                    snapshot.path_probes.insert(path.to_string(), probe);
                    Arc::make_mut(&mut snapshot.observations)
                        .insert_path_probe(path.to_string(), probe);
                    progressed = true;
                }
            }
            InputKey::RealPath { path } => {
                if !snapshot.realpaths.contains_key(path.as_ref()) {
                    let resolved = fixture.realpath_of(path);
                    snapshot
                        .realpaths
                        .insert(path.to_string(), resolved.clone());
                    Arc::make_mut(&mut snapshot.observations)
                        .insert_real_path(path.to_string(), resolved.map(Arc::from));
                    progressed = true;
                }
            }
            InputKey::PackageManifest { directory }
                if !snapshot.manifests.contains_key(directory.as_ref()) =>
            {
                let manifest = fixture
                    .manifests
                    .get(directory.as_ref())
                    .cloned()
                    .map(Arc::new);
                snapshot
                    .manifests
                    .insert(directory.to_string(), manifest.clone());
                Arc::make_mut(&mut snapshot.observations)
                    .insert_package_manifest(directory.to_string(), manifest);
                progressed = true;
            }
            _ => {}
        }
    }
    progressed
}

pub(super) struct KernelCoreRunResult {
    pub(super) result: Option<ResolveResult>,
    pub(super) resolved: Option<String>,
    pub(super) resolution_kind: Option<verter_semantic::resolver_core::ResolutionKind>,
    pub(super) ordered_selectors: Vec<ConsumedResolutionObservationKey>,
    pub(super) replayed_facts: Vec<crate::resolution_currency::ResolutionFactKey>,
    pub(super) path_probes: Vec<(String, PathProbe)>,
    pub(super) waves: u32,
}

fn completed_kernel_result(
    value: Option<ResolveResult>,
    output: &AttemptOutput,
    snapshot: &FullKernelSnapshot,
    replayed_facts: Vec<crate::resolution_currency::ResolutionFactKey>,
    waves: u32,
    scope: LedgerScope,
) -> KernelCoreRunResult {
    let witness = normalized_kernel_witness(output, snapshot);
    match scope {
        LedgerScope::ConvertedCase => assert_historical_witness(&witness),
        LedgerScope::DriverAcceptanceOutsideTheLedger => {
            let case = current_case_name();
            assert!(
                !historical_outcomes().iter().any(|row| row.name == case),
                "{case:?} carries a historical row and must run as a converted case"
            );
        }
    }
    let ordered_selectors = output.consumed_resolution_observations().to_vec();
    let path_probes = ordered_selectors
        .iter()
        .filter_map(|key| match key {
            ConsumedResolutionObservationKey::PathProbe { path } => Some((
                path.to_string(),
                *snapshot
                    .path_probes
                    .get(path.as_ref())
                    .expect("consumed probe must be loaded"),
            )),
            _ => None,
        })
        .collect();
    KernelCoreRunResult {
        resolved: value.as_ref().map(|result| result.source_id.clone()),
        resolution_kind: value.as_ref().map(|result| result.resolution_kind),
        result: value,
        ordered_selectors,
        replayed_facts,
        path_probes,
        waves,
    }
}

struct ProductionDriverRun {
    result: Option<ResolveResult>,
    replayed_facts: Vec<crate::resolution_currency::ResolutionFactKey>,
}

fn run_production_driver(
    fixture: &ResolutionFixture,
    resolver: &ModuleResolverCore,
    request: &ResolveRequest,
) -> ProductionDriverRun {
    let authority = crate::memory::MemoryWorkspace::new(Default::default());
    let world = authority
        .engine
        .capture_published_resolution_world(authority.engine.default_resolution_population())
        .expect("the in-memory workspace must publish a settled resolution world");
    let transaction = Mutex::new(crate::resolution_currency::ResolutionTransaction::new(
        world,
    ));
    let reader = FixtureReader {
        fixture,
        pending_directory_observations: StdMutex::new(Vec::new()),
    };
    let tracked = crate::resolution_currency::TransactionReader::new(&reader, &transaction);
    let capability = crate::engine::TrackedResolutionCapability::for_conversion_test();

    let mut ledger = crate::resolver::InputResolutionLedger::default();
    let result =
        crate::resolver::resolve_tracked(resolver, &capability, &tracked, &mut ledger, request)
            .unwrap_or_else(|failure| panic!("production driver failed unexpectedly: {failure:?}"));
    let replayed_facts = transaction.lock().direct_edges();
    ProductionDriverRun {
        result,
        replayed_facts,
    }
}

fn run_production_driver_for_project(
    fixture: &ResolutionFixture,
    resolver: &ModuleResolverCore,
    owner: &ProjectOwnership,
    specifier: &str,
) -> ProductionDriverRun {
    let authority = crate::memory::MemoryWorkspace::new(Default::default());
    let world = authority
        .engine
        .capture_published_resolution_world(authority.engine.default_resolution_population())
        .expect("the in-memory workspace must publish a settled resolution world");
    let transaction = Mutex::new(crate::resolution_currency::ResolutionTransaction::new(
        world,
    ));
    let reader = FixtureReader {
        fixture,
        pending_directory_observations: StdMutex::new(Vec::new()),
    };
    let tracked = crate::resolution_currency::TransactionReader::new(&reader, &transaction);
    let capability = crate::engine::TrackedResolutionCapability::for_conversion_test();
    let context = ResolutionContext {
        phase: ResolvePhase::ProviderGraph,
        kind: ResolveRequestKind::EsmImport,
    };
    let mut ledger = crate::resolver::InputResolutionLedger::default();

    let result = crate::resolver::resolve_for_project_tracked(
        resolver,
        &capability,
        &tracked,
        &mut ledger,
        owner,
        specifier,
        context,
    )
    .unwrap_or_else(|failure| panic!("production driver failed unexpectedly: {failure:?}"));
    let replayed_facts = transaction.lock().direct_edges();
    ProductionDriverRun {
        result,
        replayed_facts,
    }
}

/// Whether a run belongs to a table-backed compatibility case. Independent
/// driver-acceptance tests exercise the same helper without a table row and
/// say so explicitly at the call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LedgerScope {
    ConvertedCase,
    DriverAcceptanceOutsideTheLedger,
}

fn run_kernel_core(
    fixture: &ResolutionFixture,
    projects: Vec<IdeProjectConfig>,
    importer_id: &str,
    specifier: &str,
) -> KernelCoreRunResult {
    run_kernel_core_in(
        fixture,
        projects,
        importer_id,
        specifier,
        LedgerScope::ConvertedCase,
    )
}

pub(super) fn run_kernel_core_in(
    fixture: &ResolutionFixture,
    projects: Vec<IdeProjectConfig>,
    importer_id: &str,
    specifier: &str,
    scope: LedgerScope,
) -> KernelCoreRunResult {
    let resolver = ModuleResolverCore::new(projects);
    let request = ResolveRequest {
        importer_id: importer_id.to_string(),
        specifier: specifier.to_string(),
        phase: ResolvePhase::ProviderGraph,
        kind: ResolveRequestKind::EsmImport,
    };
    let production = run_production_driver(fixture, &resolver, &request);
    let mut snapshot = FullKernelSnapshot::default();
    let basis = kernel_basis();
    let frame = resolver.resolve_frame(&request);
    let mut waves = 0;
    loop {
        let view = build_full_attempt_view(&snapshot, basis);
        let outcome = frame.attempt(&view, basis);
        drop(view);
        match outcome {
            AttemptOutcome::Complete(CompletedAttempt { value, output }) => {
                assert_eq!(
                    value, production.result,
                    "production resolve_tracked and the semantic attempt must agree"
                );
                return completed_kernel_result(
                    value,
                    &output,
                    &snapshot,
                    production.replayed_facts,
                    waves,
                    scope,
                );
            }
            AttemptOutcome::NeedInputs(load_set) => {
                assert_eq!(load_set.basis(), basis);
                assert!(
                    load_full_snapshot(&mut snapshot, fixture, load_set.keys()),
                    "kernel made no progress for {importer_id:?} / {specifier:?}"
                );
                waves += 1;
            }
            AttemptOutcome::Terminal(failure) => {
                panic!("kernel attempt terminated unexpectedly: {failure:?}")
            }
        }
    }
}

fn run_kernel_core_for_project(
    fixture: &ResolutionFixture,
    projects: Vec<IdeProjectConfig>,
    owner: &ProjectOwnership,
    specifier: &str,
) -> KernelCoreRunResult {
    let resolver = ModuleResolverCore::new(projects);
    let production = run_production_driver_for_project(fixture, &resolver, owner, specifier);
    let context = ResolutionContext {
        phase: ResolvePhase::ProviderGraph,
        kind: ResolveRequestKind::EsmImport,
    };
    let mut snapshot = FullKernelSnapshot::default();
    let basis = kernel_basis();
    let frame = resolver.resolve_for_project_frame(owner, specifier, context);
    let mut waves = 0;
    loop {
        let view = build_full_attempt_view(&snapshot, basis);
        let outcome = frame.attempt(&view, basis);
        drop(view);
        match outcome {
            AttemptOutcome::Complete(CompletedAttempt { value, output }) => {
                assert_eq!(
                    value, production.result,
                    "production resolve_for_project_tracked and semantic attempt must agree"
                );
                return completed_kernel_result(
                    value,
                    &output,
                    &snapshot,
                    production.replayed_facts,
                    waves,
                    LedgerScope::ConvertedCase,
                );
            }
            AttemptOutcome::NeedInputs(load_set) => {
                assert_eq!(load_set.basis(), basis);
                assert!(
                    load_full_snapshot(&mut snapshot, fixture, load_set.keys()),
                    "kernel made no progress for explicit project / {specifier:?}"
                );
                waves += 1;
            }
            AttemptOutcome::Terminal(failure) => {
                panic!("explicit-project kernel attempt terminated unexpectedly: {failure:?}")
            }
        }
    }
}

pub(super) fn project(root: &str, tsconfig: &str) -> IdeProjectConfig {
    crate::resolver::ide_project_config(
        root.to_string(),
        root.to_string(),
        Some(tsconfig.to_string()),
    )
}

pub(super) fn with_aliases(
    mut project: IdeProjectConfig,
    aliases: &[(&str, &str)],
) -> IdeProjectConfig {
    project.workspace_aliases = aliases
        .iter()
        .map(
            |(find, replacement)| verter_semantic::resolver_core::WorkspaceAlias {
                find: find.to_string(),
                replacement: replacement.to_string(),
            },
        )
        .collect();
    project
}

fn with_paths(mut project: IdeProjectConfig, paths: Vec<(&str, Vec<&str>)>) -> IdeProjectConfig {
    project.compiler_options.paths = paths
        .into_iter()
        .map(|(pattern, targets)| {
            (
                pattern.to_string(),
                targets.into_iter().map(str::to_string).collect(),
            )
        })
        .collect();
    project
}

fn with_references(mut project: IdeProjectConfig, references: &[&str]) -> IdeProjectConfig {
    project.references = references
        .iter()
        .map(|reference| reference.to_string())
        .collect();
    project
}

fn empty_manifest() -> ResolutionPackageManifest {
    ResolutionPackageManifest {
        main: None,
        module: None,
        types: None,
        typings: None,
        exports: None,
        imports: None,
    }
}

#[test]
fn kernel_runner_positive_case_matches_the_witness_contract_result() {
    let fixture =
        ResolutionFixture::new(&["/p/mod.tsx"]).with_realpath("/p/mod.tsx", "/store/pkg/mod.tsx");
    let result = run_kernel_core(&fixture, Vec::new(), "/p/main.ts", "./mod.js");

    assert_eq!(result.resolved.as_deref(), Some("/store/pkg/mod.tsx"));
}

#[test]
fn kernel_runner_restarts_cleanly_on_a_basis_change() {
    let fixture =
        ResolutionFixture::new(&["/p/mod.tsx"]).with_realpath("/p/mod.tsx", "/store/pkg/mod.tsx");
    let resolver = ModuleResolverCore::new(Vec::new());
    let request = ResolveRequest {
        importer_id: "/p/main.ts".to_string(),
        specifier: "./mod.js".to_string(),
        phase: ResolvePhase::ProviderGraph,
        kind: ResolveRequestKind::EsmImport,
    };
    let mut snapshot = FullKernelSnapshot::default();
    let frame = resolver.resolve_frame(&request);
    let live_basis = Arc::new(StdMutex::new(basis_for(2)));
    let mut expected_basis = basis_for(1);
    let mut restarts = 0;

    let result = loop {
        let view = {
            let basis = *live_basis.lock().unwrap();
            build_full_attempt_view(&snapshot, basis)
        };
        let outcome = frame.attempt(&view, expected_basis);
        drop(view);
        match outcome {
            AttemptOutcome::Complete(CompletedAttempt { value, output }) => {
                // A restart must be invisible in the final witness: the
                // retained outcome is the no-restart one.
                assert_historical_witness(&normalized_kernel_witness(&output, &snapshot));
                break value;
            }
            AttemptOutcome::NeedInputs(load_set) if load_set.basis() != expected_basis => {
                snapshot = FullKernelSnapshot::default();
                expected_basis = load_set.basis();
                restarts += 1;
            }
            AttemptOutcome::NeedInputs(load_set) => {
                assert!(load_full_snapshot(&mut snapshot, &fixture, load_set.keys()));
            }
            AttemptOutcome::Terminal(failure) => {
                panic!("basis-change attempt terminated unexpectedly: {failure:?}")
            }
        }
    };

    assert_eq!(restarts, 1);
    assert_eq!(
        result.as_ref().map(|resolved| resolved.source_id.as_str()),
        Some("/store/pkg/mod.tsx")
    );
    let clean = run_kernel_core(&fixture, Vec::new(), "/p/main.ts", "./mod.js");
    assert_eq!(result, clean.result);
}

#[test]
fn kernel_runner_miss_case_matches_the_witness_contract_result() {
    let fixture = ResolutionFixture::new(&[]);
    let result = run_kernel_core(&fixture, Vec::new(), "/p/main.ts", "./missing");

    assert_eq!(result.resolved, None);
}

#[test]
fn resolution_witness_positive_case_kernel_matches_legacy() {
    let fixture =
        ResolutionFixture::new(&["/p/mod.tsx"]).with_realpath("/p/mod.tsx", "/store/pkg/mod.tsx");
    let result = run_kernel_core(&fixture, Vec::new(), "/p/main.ts", "./mod.js");

    assert_eq!(result.resolved.as_deref(), Some("/store/pkg/mod.tsx"));
    assert!(
        result
            .path_probes
            .contains(&("/p/mod.ts".to_string(), PathProbe::Absent,)),
        "the production witness must retain the rejected .ts probe"
    );
    assert_eq!(
        result
            .path_probes
            .iter()
            .map(|(path, _)| path.as_str())
            .collect::<Vec<_>>(),
        vec!["/p/mod.ts", "/p/mod.tsx"]
    );
}

#[test]
fn resolution_witness_miss_case_kernel_matches_legacy() {
    let fixture = ResolutionFixture::new(&[]);
    let result = run_kernel_core(&fixture, Vec::new(), "/p/main.ts", "./missing");

    assert_eq!(result.resolved, None);
    // The carrier half of the precedence order follows the language
    // registry, so the expected count follows it too. A frozen total here
    // is the same defect the resolver had: a number that must track a
    // registry, pinned where the registry cannot reach it.
    const NON_CARRIER_CANDIDATES: usize = 23;
    let carriers = verter_language::LanguageRegistry::global()
        .carrier_extensions()
        .len();
    assert!(
        carriers > 0,
        "the registry must declare a carrier or this asserts nothing"
    );
    assert_eq!(
        result.path_probes.len(),
        NON_CARRIER_CANDIDATES + carriers,
        "the production driver must exhaust exactly the full precedence order"
    );
}

#[test]
fn full_driver_resolves_a_relative_specifier_for_an_owned_importer() {
    // Discriminates from the narrow-probe-slice tests above: this
    // exercises `resolve_source_id`'s OWN relative branch (no
    // `package_follow_is_confirmed` guard â that's `resolve_source_id_unowned`-only)
    // through the REAL top-level entry point, not the bare probe.
    let fixture = ResolutionFixture::new(&["/proj/src/sibling.ts"]);
    let projects = vec![project("/proj", "/proj/tsconfig.json")];

    let kernel = run_kernel_core(&fixture, projects, "/proj/src/main.ts", "./sibling.ts");

    assert_eq!(kernel.resolved.as_deref(), Some("/proj/src/sibling.ts"));
    assert_eq!(
        kernel.resolution_kind,
        Some(verter_semantic::resolver_core::ResolutionKind::Relative)
    );
}

#[test]
fn full_driver_resolves_via_a_workspace_alias() {
    let fixture = ResolutionFixture::new(&["/proj/src/util.ts"]);
    let projects = vec![with_aliases(
        project("/proj", "/proj/tsconfig.json"),
        &[("@/", "/proj/src")],
    )];

    let kernel = run_kernel_core(&fixture, projects, "/proj/src/main.ts", "@/util");

    assert_eq!(kernel.resolved.as_deref(), Some("/proj/src/util.ts"));
    assert_eq!(
        kernel.resolution_kind,
        Some(verter_semantic::resolver_core::ResolutionKind::WorkspaceAlias)
    );

    // The driver
    // must genuinely iterate (manifest-miss check, then path probe, then
    // realpath, each its own NeedInputs wave), not resolve everything in
    // one pass â and the manifest check (resolve_path_mapping_target's
    // OWN structure) must be consumed strictly before the path probe,
    // which must be consumed strictly before the realpath.
    assert!(
        kernel.waves >= 3,
        "expected multiple NeedInputs waves (manifest, probe, realpath), got {}",
        kernel.waves
    );
    // The manifest-observation boundary permits two witness shapes: the kernel
    // primitive `PackageManifest { directory }`, or
    // — the miss shape — a `PathProbe` on that directory's literal
    // `package.json`. The compatibility fixture normalizes the two together,
    // and the assertion accepts both shapes. Both are pinned to THIS resolution's
    // directory, not to a suffix, so an unrelated manifest probe elsewhere on
    // the path cannot satisfy it.
    let manifest_pos = kernel.ordered_selectors.iter().position(|k| {
        matches!(
            k,
            verter_semantic::resolver_core::ConsumedResolutionObservationKey::PackageManifest {
                directory
            } if directory.as_ref() == "/proj/src/util"
        ) || matches!(
            k,
            verter_semantic::resolver_core::ConsumedResolutionObservationKey::PathProbe { path }
                if path.as_ref() == "/proj/src/util/package.json"
        )
    });
    let probe_pos = kernel.ordered_selectors.iter().position(|k| {
        matches!(
            k,
            verter_semantic::resolver_core::ConsumedResolutionObservationKey::PathProbe { path }
                if path.as_ref() == "/proj/src/util.ts"
        )
    });
    let realpath_pos = kernel.ordered_selectors.iter().position(|k| {
        matches!(
            k,
            verter_semantic::resolver_core::ConsumedResolutionObservationKey::RealPath { .. }
        )
    });
    match (manifest_pos, probe_pos, realpath_pos) {
        (Some(m), Some(p), Some(r)) => {
            assert!(
                m < p && p < r,
                "expected manifest-check < path-probe < realpath in consumed order, got \
                 manifest={m} probe={p} realpath={r}"
            );
        }
        other => panic!("expected all three selector kinds present, got {other:?}"),
    }
}

#[test]
fn full_driver_resolves_via_tsconfig_paths() {
    let fixture = ResolutionFixture::new(&["/proj/src/app/thing.ts"]);
    let projects = vec![with_paths(
        project("/proj", "/proj/tsconfig.json"),
        vec![("@app/*", vec!["./src/app/*"])],
    )];

    let kernel = run_kernel_core(&fixture, projects, "/proj/src/main.ts", "@app/thing");

    assert_eq!(kernel.resolved.as_deref(), Some("/proj/src/app/thing.ts"));
    assert_eq!(
        kernel.resolution_kind,
        Some(verter_semantic::resolver_core::ResolutionKind::TsConfigPath)
    );
}

#[test]
fn full_driver_resolves_via_the_base_url_fallback() {
    let fixture = ResolutionFixture::new(&["/proj/src2/thing.ts"]);
    let mut owner = project("/proj", "/proj/tsconfig.json");
    owner.compiler_options.base_url = Some("/proj/src2".to_string());
    let projects = vec![owner];

    let kernel = run_kernel_core(&fixture, projects, "/proj/src/main.ts", "thing");

    assert_eq!(kernel.resolved.as_deref(), Some("/proj/src2/thing.ts"));
    assert_eq!(
        kernel.resolution_kind,
        Some(verter_semantic::resolver_core::ResolutionKind::TsConfigPath)
    );
}

#[test]
fn full_driver_resolves_via_a_project_reference() {
    let fixture = ResolutionFixture::new(&["/proj/b/src/thing.ts"]);
    let project_a = with_references(
        project("/proj/a", "/proj/a/tsconfig.json"),
        &["/proj/b/tsconfig.json"],
    );
    let project_b = with_aliases(
        project("/proj/b", "/proj/b/tsconfig.json"),
        &[("@b/", "/proj/b/src")],
    );
    let projects = vec![project_a, project_b];

    let kernel = run_kernel_core(&fixture, projects, "/proj/a/src/main.ts", "@b/thing");

    assert_eq!(kernel.resolved.as_deref(), Some("/proj/b/src/thing.ts"));
    assert_eq!(
        kernel.resolution_kind,
        Some(verter_semantic::resolver_core::ResolutionKind::ProjectReference)
    );
}

#[test]
fn full_driver_a_project_reference_cycle_terminates_on_both_engines() {
    // Proves the production driver and semantic kernel terminate on
    // the same genuine project-reference cycle (not just at the free-function
    // free-function unit-test level, which already covers this in
    // isolation) â A -> B, B -> A (back-edge) and B -> C; only C's alias
    // resolves. Neither engine may infinite-loop or stack-overflow.
    let fixture = ResolutionFixture::new(&["/proj/c/src/thing.ts"]);
    let project_a = with_references(
        project("/proj/a", "/proj/a/tsconfig.json"),
        &["/proj/b/tsconfig.json"],
    );
    let project_b = with_references(
        project("/proj/b", "/proj/b/tsconfig.json"),
        &["/proj/a/tsconfig.json", "/proj/c/tsconfig.json"],
    );
    let project_c = with_aliases(
        project("/proj/c", "/proj/c/tsconfig.json"),
        &[("@c/", "/proj/c/src")],
    );
    let projects = vec![project_a, project_b, project_c];

    let kernel = run_kernel_core(&fixture, projects, "/proj/a/src/main.ts", "@c/thing");

    assert_eq!(kernel.resolved.as_deref(), Some("/proj/c/src/thing.ts"));
}

#[test]
fn full_driver_resolves_via_hash_imports() {
    let fixture = ResolutionFixture::new(&["/proj/src/utils/format.ts"]).with_manifest(
        "/proj",
        verter_semantic::resolver_core::ResolutionPackageManifest {
            imports: Some(serde_json::json!({ "#utils/*": "./src/utils/*.ts" })),
            ..empty_manifest()
        },
    );
    let projects = vec![project("/proj", "/proj/tsconfig.json")];

    let kernel = run_kernel_core(&fixture, projects, "/proj/src/main.ts", "#utils/format");

    assert_eq!(
        kernel.resolved.as_deref(),
        Some("/proj/src/utils/format.ts")
    );
    assert_eq!(
        kernel.resolution_kind,
        Some(verter_semantic::resolver_core::ResolutionKind::PackageImports)
    );
}

#[test]
fn full_driver_resolves_via_node_modules_exports_with_conditions() {
    let fixture = ResolutionFixture::new(&["/proj/node_modules/lodash/esm/index.js"])
        .with_manifest(
            "/proj/node_modules/lodash",
            verter_semantic::resolver_core::ResolutionPackageManifest {
                exports: Some(serde_json::json!({
                    ".": { "import": "./esm/index.js", "require": "./cjs/index.js" }
                })),
                ..empty_manifest()
            },
        );
    let projects = vec![project("/proj", "/proj/tsconfig.json")];

    let kernel = run_kernel_core(&fixture, projects, "/proj/src/main.ts", "lodash");

    assert_eq!(
        kernel.resolved.as_deref(),
        Some("/proj/node_modules/lodash/esm/index.js")
    );
    assert_eq!(
        kernel.resolution_kind,
        Some(verter_semantic::resolver_core::ResolutionKind::PackageExports)
    );
}

#[test]
fn full_driver_resolves_a_scoped_package_via_legacy_main_field() {
    let fixture = ResolutionFixture::new(&["/proj/node_modules/@scope/pkg/index.js"])
        .with_manifest(
            "/proj/node_modules/@scope/pkg",
            verter_semantic::resolver_core::ResolutionPackageManifest {
                main: Some("./index.js".to_string()),
                ..empty_manifest()
            },
        );
    let projects = vec![project("/proj", "/proj/tsconfig.json")];

    let kernel = run_kernel_core(&fixture, projects, "/proj/src/main.ts", "@scope/pkg");

    assert_eq!(
        kernel.resolved.as_deref(),
        Some("/proj/node_modules/@scope/pkg/index.js")
    );
    assert_eq!(
        kernel.resolution_kind,
        Some(verter_semantic::resolver_core::ResolutionKind::NodeModules)
    );
}

#[test]
fn full_driver_resolves_via_explicit_project_ownership() {
    let fixture = ResolutionFixture::new(&["/proj/src/thing.ts"]);
    let projects = vec![with_aliases(
        project("/proj", "/proj/tsconfig.json"),
        &[("@/", "/proj/src")],
    )];
    let owner = verter_semantic::resolver_core::ProjectOwnership {
        project_root: "/proj".to_string(),
        tsconfig_path: Some("/proj/tsconfig.json".to_string()),
    };

    let kernel = run_kernel_core_for_project(&fixture, projects, &owner, "@/thing");

    assert_eq!(kernel.resolved.as_deref(), Some("/proj/src/thing.ts"));
}

#[test]
fn full_driver_owner_overlap_selects_the_nearest_root() {
    let fixture = ResolutionFixture::new(&["/proj/pkg/INNER/thing.ts"]);
    let outer = with_aliases(
        project("/proj", "/proj/tsconfig.json"),
        &[("@/", "/proj/OUTER")],
    );
    let inner = with_aliases(
        project("/proj/pkg", "/proj/pkg/tsconfig.json"),
        &[("@/", "/proj/pkg/INNER")],
    );
    let projects = vec![outer, inner];

    let kernel = run_kernel_core(&fixture, projects, "/proj/pkg/src/main.ts", "@/thing");

    // Discriminates: if the nearest-root selection diverged (e.g. picked
    // the outer project), this would resolve "/proj/OUTER/thing.ts"
    // instead â which is absent from the fixture, so a divergent
    // selection would show up as a miss, not a silently-wrong hit.
    assert_eq!(kernel.resolved.as_deref(), Some("/proj/pkg/INNER/thing.ts"));
}

#[test]
fn full_driver_a_full_chain_miss_agrees_on_both_engines() {
    let fixture = ResolutionFixture::new(&[]);
    let projects = vec![with_paths(
        with_aliases(
            project("/proj", "/proj/tsconfig.json"),
            &[("@/", "/proj/nowhere")],
        ),
        vec![("@app/*", vec!["./missing/*"])],
    )];

    let kernel = run_kernel_core(
        &fixture,
        projects,
        "/proj/src/main.ts",
        "totally-unresolvable-xyz",
    );

    assert_eq!(kernel.resolved, None);
}

// Additional resolution-shape coverage.

#[test]
fn full_driver_resolves_an_absolute_specifier_for_an_owned_importer() {
    let fixture = ResolutionFixture::new(&["/abs/target.ts"]);
    let projects = vec![project("/proj", "/proj/tsconfig.json")];

    let kernel = run_kernel_core(&fixture, projects, "/proj/src/main.ts", "/abs/target.ts");

    assert_eq!(kernel.resolved.as_deref(), Some("/abs/target.ts"));
    assert_eq!(
        kernel.resolution_kind,
        Some(verter_semantic::resolver_core::ResolutionKind::Relative)
    );
}

#[test]
fn full_driver_workspace_alias_wins_over_tsconfig_paths_and_base_url() {
    // "@/thing" matches the alias ("@/" -> "/proj/alias") AND would also
    // match a tsconfig `paths` pattern and the `baseUrl` fallback if
    // either were tried â `resolve_source_id`'s own declared order
    // (aliases, THEN paths, THEN baseUrl) means the alias must win
    // outright. All three targets exist in the fixture, so a wrong
    // precedence would silently resolve to the WRONG file instead of
    // missing.
    let fixture = ResolutionFixture::new(&[
        "/proj/alias/thing.ts",
        "/proj/paths-target/thing.ts",
        "/proj/base/thing.ts",
    ]);
    let mut owner = with_paths(
        with_aliases(
            project("/proj", "/proj/tsconfig.json"),
            &[("@/", "/proj/alias")],
        ),
        vec![("@/*", vec!["./paths-target/*"])],
    );
    owner.compiler_options.base_url = Some("/proj/base".to_string());
    let projects = vec![owner];

    let kernel = run_kernel_core(&fixture, projects, "/proj/src/main.ts", "@/thing");

    assert_eq!(kernel.resolved.as_deref(), Some("/proj/alias/thing.ts"));
    assert_eq!(
        kernel.resolution_kind,
        Some(verter_semantic::resolver_core::ResolutionKind::WorkspaceAlias)
    );
}

#[test]
fn full_driver_a_dangling_project_reference_falls_through_without_panicking() {
    // The importer's ONLY reference names a tsconfig with no matching
    // config in `projects` â the reference is dangling. The production driver
    // may panic on this; both must simply fall through to an eventual
    // miss (nothing else in this fixture resolves either).
    let fixture = ResolutionFixture::new(&[]);
    let projects = vec![with_references(
        project("/proj", "/proj/tsconfig.json"),
        &["/proj/missing/tsconfig.json"],
    )];

    let kernel = run_kernel_core(
        &fixture,
        projects,
        "/proj/src/main.ts",
        "unresolvable-specifier",
    );

    assert_eq!(kernel.resolved, None);
}

#[test]
fn full_driver_resolves_via_node_modules_exports_array_form() {
    let fixture = ResolutionFixture::new(&["/proj/node_modules/pkg/dist/second.js"]).with_manifest(
        "/proj/node_modules/pkg",
        verter_semantic::resolver_core::ResolutionPackageManifest {
            exports: Some(serde_json::json!({ ".": ["./dist/first.js", "./dist/second.js"] })),
            ..empty_manifest()
        },
    );
    let projects = vec![project("/proj", "/proj/tsconfig.json")];

    let kernel = run_kernel_core(&fixture, projects, "/proj/src/main.ts", "pkg");

    assert_eq!(
        kernel.resolved.as_deref(),
        Some("/proj/node_modules/pkg/dist/second.js")
    );
    assert_eq!(
        kernel.resolution_kind,
        Some(verter_semantic::resolver_core::ResolutionKind::PackageExports)
    );
}

#[test]
fn full_driver_carrier_import_provider_projection_matches_legacy_end_to_end() {
    let fixture = ResolutionFixture::new(&["/proj/src/Comp.vue"]);
    let projects = vec![project("/proj", "/proj/tsconfig.json")];
    let result = run_kernel_core(&fixture, projects, "/proj/src/Parent.vue", "./Comp.vue")
        .result
        .expect("the production driver must resolve the carrier import");

    // The complete DTO must equal the table-backed reference DTO.
    assert_historical_outcome(OutcomeKind::Dto, &format!("{result:?}"));
    assert_eq!(result.source_id, "/proj/src/Comp.vue");
    assert_eq!(result.provider_id, "/proj/src/Comp.vue.verter.ts");
    assert_eq!(
        result.provider_target,
        verter_semantic::resolver_core::ProviderTarget::CarrierPublicApi
    );
    assert_eq!(result.provider_specifier, "./Comp.vue.verter.ts");
    assert_eq!(
        result.owner_tsconfig_path.as_deref(),
        Some("/proj/tsconfig.json")
    );
    assert_eq!(
        result.resolution_kind,
        verter_semantic::resolver_core::ResolutionKind::Relative
    );
}

#[test]
fn full_driver_preferred_specifier_candidates_agrees_with_legacy() {
    let projects = vec![with_paths(
        with_aliases(
            project("/proj", "/proj/tsconfig.json"),
            &[("@/", "/proj/src")],
        ),
        vec![("@app/*", vec!["./src/app/*"])],
    )];
    let core = ModuleResolverCore::new(projects);

    let candidates =
        core.preferred_specifier_candidates("/proj/src/main.ts", "/proj/src/app/thing.ts");

    assert_historical_outcome(OutcomeKind::Candidates, &format!("{candidates:?}"));
    assert_eq!(
        candidates,
        Some(vec![
            "@app/thing.ts".to_string(),
            "@/app/thing.ts".to_string(),
        ])
    );
    assert!(
        candidates.is_some(),
        "sanity: the importer is owned, candidates must be Some"
    );
}

#[test]
fn full_driver_project_exact_result_agrees_with_legacy() {
    let core = ModuleResolverCore::new(vec![project("/proj", "/proj/tsconfig.json")]);
    let context = ResolutionContext {
        phase: ResolvePhase::ProviderGraph,
        kind: ResolveRequestKind::EsmImport,
    };

    let result = core.project_exact_result(
        "/proj/src/main.ts",
        "whatever",
        "/proj/src/exact.ts".to_string(),
        context,
    );

    assert_historical_outcome(OutcomeKind::Dto, &format!("{result:?}"));
    assert_eq!(result.source_id, "/proj/src/exact.ts");
    assert_eq!(result.provider_id, "/proj/src/exact.ts");
    assert_eq!(result.provider_specifier, "whatever");
    assert_eq!(
        result.provider_target,
        verter_semantic::resolver_core::ProviderTarget::ShadowSourceFile
    );
    assert_eq!(
        result.owner_tsconfig_path.as_deref(),
        Some("/proj/tsconfig.json")
    );
    assert_eq!(
        result.resolution_kind,
        verter_semantic::resolver_core::ResolutionKind::Bundler
    );
}

// ── Compatibility-table integrity: every case and allowed carrier-probe delta is explicit ──

#[test]
fn historical_outcome_ledger_is_total_and_unique() {
    let rows = historical_outcomes();
    let mut names: Vec<&str> = rows.iter().map(|row| row.name).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(
        names.len(),
        CONVERTED_CASE_COUNT,
        "every converted case carries a captured historical outcome, and no row names a case \
         that was not converted"
    );
    let mut keys: Vec<(&str, &str)> = rows.iter().map(|row| (row.name, row.kind)).collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(
        keys.len(),
        rows.len(),
        "a case may carry at most one row per outcome kind"
    );
    for row in &rows {
        assert!(
            matches!(row.kind, "witness" | "dto" | "candidates"),
            "row {} has unknown kind {:?}",
            row.name,
            row.kind
        );
        assert!(
            !row.historical.is_empty(),
            "row {} has no captured historical value",
            row.name
        );
        assert!(
            row.current == "=" || row.current != row.historical,
            "row {} spells out a current value identical to its historical one; write `=`",
            row.name
        );
    }
}

fn witness_facts(rendered: &str) -> BTreeSet<String> {
    let inner = rendered
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .unwrap_or_else(|| panic!("a witness renders as a braced set, got {rendered:?}"));
    if inner.is_empty() {
        return BTreeSet::new();
    }
    // Facts are flat structs whose only nested payload is `Some("...")`, so
    // the `}, ` separator is unambiguous.
    inner
        .split("}, ")
        .map(|fact| {
            if fact.ends_with('}') {
                fact.to_string()
            } else {
                format!("{fact}}}")
            }
        })
        .collect()
}

#[test]
fn every_historical_delta_is_exactly_the_registered_carrier_probe_set() {
    let carriers: BTreeSet<String> = verter_language::LanguageRegistry::global()
        .carrier_extensions()
        .iter()
        .map(|ext| ext.to_string())
        .collect();
    assert!(
        carriers.len() > 1,
        "this check discriminates only with a non-Vue carrier registered"
    );

    let mut deviating_rows = 0;
    for row in historical_outcomes() {
        if row.current == "=" {
            continue;
        }
        deviating_rows += 1;
        assert_eq!(
            row.kind, "witness",
            "only witnesses may deviate from history; {} does",
            row.name
        );
        let historical = witness_facts(row.historical);
        let current = witness_facts(row.current);
        let removed: Vec<&String> = historical.difference(&current).collect();
        assert!(
            removed.is_empty(),
            "{}: the driver dropped required reference facts: {removed:?}",
            row.name
        );
        let added: Vec<&String> = current.difference(&historical).collect();
        assert!(
            !added.is_empty(),
            "{}: deviating row adds nothing",
            row.name
        );
        for fact in added {
            let canonical = fact
                .strip_prefix("PathProbe { canonical: \"")
                .and_then(|rest| rest.strip_suffix("\", outcome: Absent }"))
                .unwrap_or_else(|| {
                    panic!(
                        "{}: added fact is not an absent path probe: {fact}",
                        row.name
                    )
                });
            let (stem, ext) = canonical
                .rsplit_once('.')
                .unwrap_or_else(|| panic!("{}: probe has no extension: {canonical}", row.name));
            assert!(
                carriers.contains(ext),
                "{}: added probe {canonical} is not a registered carrier extension",
                row.name
            );
            let vue_twin = format!("PathProbe {{ canonical: \"{stem}.vue\", outcome: Absent }}");
            assert!(
                historical.contains(&vue_twin),
                "{}: the reference has no `.vue` probe at {stem}, so {canonical} is not the \
                 carrier-extension widening",
                row.name
            );
        }
    }
    assert_eq!(
        deviating_rows, 7,
        "exactly the seven miss-exhausting cases widen their probe set; a different count is a \
         new deviation"
    );
}
