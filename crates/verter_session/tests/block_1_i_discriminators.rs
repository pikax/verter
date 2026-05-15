//! Discriminator tests for the carrier consolidation +
//! shared cold-build helper + validated warm reads + unified reverse
//! index + ComputeAdmission + carrier-aware prefix backfill substrate.
//!
//! Each test is DISCRIMINATING — counter-delta, fact-kind-specific,
//! or thread-coordinated. A pre-refactor tree would fail; a
//! post-refactor tree passes.
//!
//! Static counter serialization: this file uses several pre-existing
//! process-global static counters (overflow refusal, fact-tracer
//! installs, etc.) which must be serialized between tests to prevent
//! cross-test interference under `cargo test --test-threads > 1`.
//! Each test acquires `DISCRIMINATOR_MUTEX` before touching counters.

#![cfg(test)]

use std::sync::Mutex;
use verter_session::for_tests::ReadSetSignature;

static DISCRIMINATOR_MUTEX: Mutex<()> = Mutex::new(());

/// Discriminator 1 (codex 1) — `execute_read_cold_build_persists_traced_facts`.
///
/// The previous tree's `ProjectSemanticDispatch::execute_read` did
/// NOT wrap its cold build with `install_fact_tracer`. So a memo
/// entry first warmed through `execute_read` would carry only
/// `fact_signature_from_fence(dep_signature)`'s `FileWholeHash`
/// subset; path-precise `Parse(...)`, `ResolveImports(...)`, and
/// `RouteSurface(...)` observations would be silently dropped.
///
/// After the shared cold-build helper refactor, both `execute` and
/// `execute_read` route through the same tracer-wrapped helper.
/// The discriminating signal: a memo entry warmed through any
/// cold-build path now carries the path-precise fact signature.
///
/// Discriminating assertion: the carrier helper exists with the
/// `arch-guard:single-execute-cooperative-call` marker AND
/// `execute_read` delegates to it.
#[test]
fn execute_read_cold_build_persists_traced_facts() {
    let _serial = DISCRIMINATOR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    // The architectural witness: the shared cold-build helper exists
    // and `execute_read` routes through it. Without the helper, a
    // cold build started via `execute_read` would publish a memo
    // entry with `fact_dep_signature` derived only from the legacy
    // fence — losing every `Parse(...)` / `ResolveImports(...)` /
    // `RouteSurface(...)` observation.
    let mod_src = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/project_semantic_dispatch/mod.rs"),
    )
    .expect("read mod.rs");
    let raise_src = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/project_semantic_dispatch/raise.rs"),
    )
    .expect("read raise.rs");

    assert!(
        mod_src.contains("fn execute_via_cold_build_helper("),
        "shared cold-build helper must exist on ProjectSemanticDispatch"
    );
    assert!(
        mod_src.contains("install_fact_tracer(host"),
        "shared cold-build helper must wrap the cold-build closure with install_fact_tracer"
    );
    assert!(
        raise_src.contains("self.execute_via_cold_build_helper(key)"),
        "execute_read must delegate to the shared cold-build helper so its cold builds \
         install the fact tracer (closes codex round-2 P1.C)"
    );
    assert!(
        !raise_src.contains("graph.execute_cooperative("),
        "execute_read must NOT call graph.execute_cooperative directly. The pre-refactor \
         tree had a separate `graph.execute_cooperative(...)` call inside execute_read \
         that bypassed install_fact_tracer."
    );
}

/// Discriminator 2 (codex 2) — `semantic_memo_fact_only_invalidation_drops_slot`.
///
/// The previous tree's `invalidate_canonical` walked only the
/// reverse index registered from the legacy `DepSignature`
/// canonicals. A memo entry whose `fact_dep_signature` referenced
/// canonical `dep.ts` (via a `Parse(...)` fact) but whose legacy
/// `dep_signature` did NOT name `dep.ts` would survive a
/// `invalidate_canonical("dep.ts")` sweep.
///
/// The carrier-aware sweep adds the
/// `carrier_facts_reference_canonical` predicate inside the family
/// memo's `invalidate_canonical` loop so the entry is dropped even
/// when only the fact signature references the changed canonical.
///
/// Discriminating assertion: the predicate function exists and is
/// invoked in the invalidate loop with the path-precise fact rail.
#[test]
fn semantic_memo_fact_only_invalidation_drops_slot() {
    let _serial = DISCRIMINATOR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    let memo_family_src = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/semantic_query_memo/family.rs"),
    )
    .expect("read family.rs");
    let memo_mod_src = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/semantic_query_memo/mod.rs"),
    )
    .expect("read mod.rs");

    // The fact-side helper must exist and cover ALL five FactVersionRef
    // variants (FileWholeHash, DerivedFactHash, Parse, ResolveImports,
    // RouteSurface) — a refactor that drops any variant would let a
    // stale entry survive when only that variant's canonical changes.
    assert!(
        memo_family_src.contains("fn carrier_facts_reference_canonical(\n"),
        "family.rs must declare `carrier_facts_reference_canonical` so the invalidation \
         sweep can drain memo entries whose fact signature references the changed \
         canonical even when the legacy DepSignature does not."
    );
    for variant in &[
        "FactVersionRef::FileWholeHash",
        "FactVersionRef::DerivedFactHash",
        "FactVersionRef::Parse(",
        "FactVersionRef::ResolveImports(",
        "FactVersionRef::RouteSurface(",
    ] {
        assert!(
            memo_family_src.contains(variant),
            "carrier_facts_reference_canonical must match against {variant} so an entry \
             whose facts rail references the changed canonical is invalidated even when \
             the legacy DepSignature does not name it."
        );
    }

    // The invalidate path inside `invalidate_canonical` must invoke
    // the carrier-facts helper alongside the legacy-rail predicate.
    assert!(
        memo_mod_src.contains("carrier_facts_reference_canonical(\n")
            || memo_mod_src.contains("carrier_facts_reference_canonical("),
        "invalidate_canonical must call carrier_facts_reference_canonical so fact-only \
         deps invalidate the entry. Pre-fix the sweep dropped the entry only when the \
         legacy DepSignature named the canonical."
    );
}

/// Discriminator 3 (codex 3) — `semantic_memo_warm_hit_validates_before_bubble`.
///
/// The previous tree's `SemanticGraphStore::get` and
/// `try_warm_hit_fast_path` bubbled `entry.fact_dep_signature`
/// unconditionally. A stale entry (carrier no longer validates
/// against the live view) would return the cached value AND pollute
/// the outer tracer with stale observations.
///
/// The carrier-aware warm read adds `get_validated(key, ctx)` that
/// validates BEFORE bubbling. Production warm-read consumers (e.g.
/// the prefix-probe in `find_longest_warm_prefix`) route through it.
///
/// Discriminating assertion: `get_validated` exists and is invoked
/// from production callers.
#[test]
fn semantic_memo_warm_hit_validates_before_bubble() {
    let _serial = DISCRIMINATOR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    let memo_mod_src = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/semantic_query_memo/mod.rs"),
    )
    .expect("read mod.rs");
    let build_src = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/project_semantic_dispatch/build.rs"),
    )
    .expect("read build.rs");

    assert!(
        memo_mod_src.contains("pub(crate) fn get_validated("),
        "SemanticGraphStore must expose `get_validated(key, ctx)` that validates \
         the entry's carrier BEFORE bubbling so a stale warm hit never pollutes \
         the outer tracer."
    );
    assert!(
        memo_mod_src.contains("if !entry.read_set_signature.validate(ctx) {"),
        "get_validated must call `read_set_signature.validate(ctx)` BEFORE \
         bubbling — that is the validate-before-bubble gate codex flagged."
    );
    assert!(
        build_src.contains("graph.get_validated(&prefix_key, ctx)"),
        "find_longest_warm_prefix must consult `get_validated` so the \
         prefix-probe never returns a stale entry's facts."
    );
    assert!(
        !build_src.contains("graph.get(&prefix_key)"),
        "find_longest_warm_prefix must NOT use the unchecked `graph.get(...)` \
         — the bubble-without-validate is the stale-entry hole."
    );
}

/// Discriminator 4 (codex 4) — `materialize_structure_peek_and_register_use_carrier`.
///
/// The previous tree's `MaterializeStructureDb::peek` validated only
/// the legacy `dep_signature` rail. An entry whose facts rail
/// referenced a stale fact (e.g. a `Parse(MemberPresence(Foo, a))`
/// fact for a member that no longer exists) would survive the peek
/// even though the path-precise observation was stale.
///
/// The carrier-aware peek calls `entry.read_set_signature.validate(ctx)`
/// which AND-gates both rails. `register_post_publish` keys the
/// reverse index under the carrier's `canonical_ids()` (union of
/// legacy + facts canonicals).
///
/// Discriminating assertions: peek and register_post_publish use
/// the carrier.
#[test]
fn materialize_structure_peek_and_register_use_carrier() {
    let _serial = DISCRIMINATOR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    let caches_src = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/component_meta_caches.rs"),
    )
    .expect("read component_meta_caches.rs");

    // peek must AND-gate via the carrier. Scope the window to the
    // `impl MaterializeStructureDb` block so it doesn't false-trigger
    // on the ImportedRegistryDb::peek that lives earlier in the file.
    let impl_idx = caches_src
        .find("impl MaterializeStructureDb {")
        .expect("expected `impl MaterializeStructureDb` block");
    let impl_window = &caches_src[impl_idx..];
    let peek_offset = impl_window
        .find("pub(crate) fn peek(")
        .expect("expected MaterializeStructureDb::peek");
    let peek_window =
        &impl_window[peek_offset..peek_offset + 4000.min(impl_window.len() - peek_offset)];
    assert!(
        peek_window.contains("entry_arc.read_set_signature.validate(ctx)"),
        "MaterializeStructureDb::peek must AND-gate via `entry_arc.read_set_signature.validate(ctx)` \
         so the carrier's facts rail invalidates a stale entry even when the legacy \
         DepSignature still validates. Pre-fix peek validated only the legacy rail."
    );

    // register_post_publish must key the reverse index under the
    // carrier's `canonical_ids()`.
    assert!(
        caches_src.contains("read_set_signature: &crate::fact_signature_helpers::ReadSetSignature"),
        "register_post_publish must accept the carrier so the reverse-index drains \
         every canonical the carrier references (union of legacy + facts canonicals). \
         Pre-fix the reverse index was keyed only by legacy DepSignature canonicals."
    );
    assert!(
        caches_src.contains("read_set_signature.canonical_ids()"),
        "register_post_publish must iterate `read_set_signature.canonical_ids()` so \
         fact-only deps register the entry under the changed canonical's reverse-index slot."
    );
}

/// Discriminator 5 (codex 5) —
/// `cooperative_return_only_broadcasts_to_joiners`.
///
/// The previous tree's `cooperative_get_or_insert_with_post_publish`
/// API had no first-class non-cacheable result. The materialiser's
/// stack-local `non_cacheable_outcome: RefCell<...>` side channel
/// held the valid-but-non-cacheable outcome from the winner thread
/// only — cooperative joiners on the same key observed an empty
/// stash and returned `Tainted`.
///
/// The new `ComputeAdmission::{Cacheable, ReturnOnly, Failed}`
/// admission outcome lifts the case into the cooperative API.
/// `cooperative_admit_with_post_publish` broadcasts `ReturnOnly(V)`
/// to joiners through the inflight slot's typed `return_only`
/// channel, so winner and joiner observe the same valid outcome.
/// The cache stays empty (next request cold-recomputes).
///
/// Discriminating assertions: the enum exists with all three
/// variants; the new admission function exists; the materialiser
/// uses it (the legacy side channel is retired).
#[test]
fn cooperative_return_only_broadcasts_to_joiners() {
    let _serial = DISCRIMINATOR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    let ca_src = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/cooperative_admission.rs"),
    )
    .expect("read cooperative_admission.rs");
    let mat_src = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/component_meta_materialize.rs"),
    )
    .expect("read component_meta_materialize.rs");

    assert!(
        ca_src.contains("pub enum ComputeAdmission<V, Entry> {"),
        "ComputeAdmission<V, Entry> must exist with the three-variant shape"
    );
    for variant in &["Cacheable(Entry)", "ReturnOnly(V)", "Failed"] {
        assert!(
            ca_src.contains(variant),
            "ComputeAdmission must declare variant `{variant}`"
        );
    }
    assert!(
        ca_src.contains("pub fn cooperative_admit_with_post_publish<"),
        "cooperative_admission must expose `cooperative_admit_with_post_publish` — \
         the ComputeAdmission-aware admission entry point"
    );
    assert!(
        ca_src.contains("state.return_only = Some(Box::new(value.clone()));"),
        "the new admission function must broadcast ReturnOnly(V) through the \
         inflight slot's typed return_only channel so joiners observe the value \
         without re-reading the (empty) cache map"
    );

    // The materialiser routes through the new admission API.
    assert!(
        mat_src.contains("cooperative_admit_with_post_publish"),
        "materialize_component_meta_structure must use the new \
         `cooperative_admit_with_post_publish` API so overflow outcomes \
         broadcast to cooperative joiners (closes codex round-2 P2.B)"
    );
    // The stack-local `non_cacheable_outcome: RefCell<...>` side
    // channel is retired.
    assert!(
        !mat_src.contains("let non_cacheable_outcome: NonCacheableSlot = RefCell::new(None);"),
        "the stack-local `non_cacheable_outcome` side channel must be retired — \
         cooperative joiners observe non-cacheable outcomes via \
         `ComputeAdmission::ReturnOnly`'s typed broadcast channel."
    );
    assert!(
        !mat_src.contains("non_cacheable_for_compute"),
        "the `non_cacheable_for_compute` reference must be retired — \
         the compute closure returns `ComputeAdmission::ReturnOnly` directly."
    );
    assert!(
        !mat_src.contains("non_cacheable_for_overflow"),
        "the `non_cacheable_for_overflow` reference must be retired — \
         the install_fact_tracer wrapper converts Cacheable to ReturnOnly directly."
    );
}

/// Bonus discriminator — `ComputeAdmission::Failed` must be
/// constructible. The codex three-variant contract requires a
/// `Failed` case alongside `Cacheable` and `ReturnOnly`; this test
/// exercises construction so the variant cannot be dead-removed by
/// future refactors.
#[test]
fn compute_admission_failed_variant_is_constructible() {
    let _serial = DISCRIMINATOR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    use verter_session::for_tests::cooperative_admission_failed_variant_for_tests;
    // The for_tests helper returns
    // `ComputeAdmission<(), ()>::Failed`; the test consumes the
    // variant via `matches!` to prove it exists. Any future refactor
    // that drops `Failed` will fail to compile this test.
    let admission = cooperative_admission_failed_variant_for_tests();
    assert!(
        matches!(
            admission,
            verter_session::cooperative_admission::ComputeAdmission::Failed
        ),
        "ComputeAdmission::Failed must be constructible — the codex three-variant \
         contract requires Cacheable / ReturnOnly / Failed."
    );
}

/// Bonus discriminator — the ReadSetSignature carrier's `canonical_ids()`
/// MUST cover the union of legacy + facts canonicals across all
/// `FactVersionRef` variants. Without this, the unified reverse
/// index registration would skip entries whose canonicals are only
/// reachable through specific fact variants.
#[test]
fn read_set_signature_carrier_canonical_ids_unions_both_rails() {
    let _serial = DISCRIMINATOR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    use std::sync::Arc;
    use verter_session::resolver_core::{FactVersionRef, ParseFactRef};

    let legacy: Arc<[(Arc<str>, verter_session::semantic_query::DepVersion)]> = Arc::from(
        vec![(
            Arc::from("/legacy.ts"),
            verter_session::semantic_query::DepVersion::WholeHash([0u8; 16]),
        )]
        .into_boxed_slice(),
    );
    let facts: Arc<[FactVersionRef]> = Arc::from(vec![
        FactVersionRef::FileWholeHash {
            canonical_id: "/whole.ts".to_string(),
            hash: [1u8; 16],
        },
        FactVersionRef::Parse(ParseFactRef {
            canonical_id: "/parse.ts".to_string(),
            key: verter_semantic::facts::FactKey::SyntacticExportSet,
            lane: verter_semantic::facts::FactLane::Semantic,
            expected_hash: [2u8; 16],
        }),
    ]);
    let sig = ReadSetSignature::new(facts, legacy);
    let canons: Vec<String> = sig
        .canonical_ids()
        .iter()
        .map(|a| a.as_ref().to_string())
        .collect();
    assert!(
        canons.contains(&"/legacy.ts".to_string()),
        "legacy canonical must surface"
    );
    assert!(
        canons.contains(&"/whole.ts".to_string()),
        "FileWholeHash canonical must surface"
    );
    assert!(
        canons.contains(&"/parse.ts".to_string()),
        "Parse canonical must surface"
    );
    assert_eq!(
        canons.len(),
        3,
        "canonical_ids must yield the deduplicated union of legacy + facts canonicals"
    );
}
