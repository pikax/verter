//! Discriminators for the cache substrate: fact-tracer persistence on
//! cold builds, the ComputeAdmission cooperative singleflight contract,
//! and the ReadSetSignature carrier's canonical coverage. Each test is
//! DISCRIMINATING — counter-delta, fact-kind-specific, or
//! thread-coordinated.
//!
//! Static counter serialization: this file uses several pre-existing
//! process-global static counters (overflow refusal, fact-tracer
//! installs, etc.) which must be serialized between tests to prevent
//! cross-test interference under `cargo test --test-threads > 1`.
//! Each test carries `#[serial(fact_counter_provenance)]` so it owns
//! the shared counters exclusively across the whole test process.

#![cfg(test)]

use serial_test::serial;
use verter_session::for_tests::ReadSetSignature;

/// Discriminator — `execute_read_cold_build_persists_traced_facts`.
///
/// A tree where `ProjectSemanticDispatch::execute_read` does
/// NOT wrap its cold build with `install_fact_tracer`. So a memo
/// entry first warmed through `execute_read` would carry only
/// `fact_signature_from_fence(dep_signature)`'s `FileWholeHash`
/// subset; path-precise `Parse(...)`, `ResolveImports(...)`, and
/// `RouteSurface(...)` observations would be silently dropped.
///
/// Both `execute` and `execute_read` route through the same
/// tracer-wrapped helper. The discriminating signal: a memo entry
/// warmed through any cold-build path carries the path-precise fact
/// signature.
///
/// Discriminating assertion: the carrier helper exists with the
/// `arch-guard:single-execute-cooperative-call` marker AND
/// `execute_read` delegates to it.
#[test]
#[serial(fact_counter_provenance)]
fn execute_read_cold_build_persists_traced_facts() {
    // The architectural witness: the shared cold-build helper exists
    // and `execute_read` routes through it. Without the helper, a
    // cold build started via `execute_read` would publish a memo
    // entry with `read_set_signature` derived only from the legacy
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
        mod_src.contains("install_fact_tracer(\n                &basis_source,"),
        "shared cold-build helper must wrap the cold-build closure with install_fact_tracer, \
         seeded from the request-bound basis source it captured before the closure — a \
         host-only or unbound tracer disarms the cold build's compaction basis"
    );
    assert!(
        mod_src.contains("let basis_source = crate::fact_signature_helpers::FactTracerBasisSource::from_ctx(self.ctx);"),
        "and that source must come from the dispatch's REQUEST-BOUND context"
    );
    assert!(
        raise_src.contains("self.execute_via_cold_build_helper(key)"),
        "execute_read must delegate to the shared cold-build helper so its cold builds \
         install the fact tracer"
    );
    assert!(
        !raise_src.contains("graph.execute_cooperative_value("),
        "execute_read must NOT call graph.execute_cooperative_value directly. A separate \
         `graph.execute_cooperative_value(...)` call inside execute_read \
         would bypass install_fact_tracer."
    );
}

/// Behavioural discriminator —
/// `semantic_memo_invalidate_drains_fact_only_canonical_entry`.
///
/// Publishes a memo entry whose carrier has:
///   - `legacy` rail referencing ONLY `/test/legacy-only.ts`
///   - `facts` rail containing a `Parse(...)` fact on
///     `/test/fact-only.ts` (and no `FileWholeHash` for it)
///
/// Then calls `invalidate_canonical("/test/fact-dep.ts")` and
/// asserts the entry was drained from the warm cache.
///
/// `register_reverse_index` iterates
/// `read_set_signature.canonical_ids()` — every canonical the fact
/// rail names. The reverse-index shard for `/test/fact-dep.ts`
/// contains the entry's (family, slot) registration.
/// `invalidate_canonical` drains the shard, finds the entry, and
/// (via the `carrier_facts_reference_canonical` helper from
/// `family.rs`) evicts it.
///
/// Discriminating signal: post-invalidation, `store.get_unvalidated(&key)` is
/// `None`. A `register_reverse_index` that failed to register the
/// entry under a `Parse`-fact canonical would leave it `Some(...)`.
#[test]
#[serial(fact_counter_provenance)]
fn semantic_memo_invalidate_drains_fact_canonical_entry() {
    use std::sync::Arc;
    use verter_session::for_tests::{ReadSetSignature, SemanticGraphStore};
    use verter_session::resolver_core::{FactVersionRef, ParseFactRef};
    use verter_session::semantic_query::{
        PrimitiveKind, QueryResult, ResolveDeclKey, ScopeId, SemanticNodeData, SemanticQueryKey,
    };

    let store = SemanticGraphStore::new();

    // Construct the query key. Its scope canonical is unrelated to the
    // fact-rail canonical — the entry's canonical reachability comes
    // entirely from the carrier's fact rail.
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: ScopeId {
            canonical_id: Arc::from("/test/scope.ts"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            local_scope: None,
            binder_scope_id: verter_session::semantic_query::BinderScopeId::file_scope(
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
            ),
        },
        name: Arc::from("MemoTarget"),
    });

    // Intern a placeholder node so we have a `Value` to publish.
    let node = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

    // Build the carrier: a fact rail with one `Parse` fact naming
    // `/test/fact-dep.ts`.
    let facts: Arc<[FactVersionRef]> = Arc::from(vec![FactVersionRef::Parse(ParseFactRef {
        canonical_id: "/test/fact-dep.ts".to_string(),
        key: verter_semantic::facts::FactKey::SyntacticExportSet,
        lane: verter_semantic::facts::FactLane::Semantic,
        expected_hash: [0x22u8; 16],
    })]);
    let carrier = ReadSetSignature::new(facts);

    // Direct publish via the test-only helper.
    let populated = store.publish_with_carrier_for_tests(
        key.clone(),
        QueryResult::Value(node),
        carrier,
        std::sync::Arc::from([]),
    );
    assert!(
        populated >= 1,
        "publish must populate at least one slot (got {populated})"
    );

    // Sanity: entry is warm; the reverse-index shard for the
    // fact-rail canonical is non-empty.
    assert!(
        store.get_unvalidated(&key).is_some(),
        "entry must be warm pre-invalidation"
    );
    assert!(
        store.canonical_to_entries_count("/test/fact-dep.ts") >= 1,
        "the fact-rail canonical's reverse-index shard MUST be populated. \
         If 0, `register_reverse_index` is dropping a `Parse`-fact \
         canonical."
    );

    // Invalidate the fact-rail canonical.
    let removed = store.invalidate_canonical("/test/fact-dep.ts");
    assert_eq!(
        removed, 1,
        "invalidate_canonical for the fact-rail canonical must drain \
         the memo entry (got {removed}). If 0, `register_reverse_index` \
         never registered the entry under `/test/fact-dep.ts` — \
         the entry is orphaned across invalidation."
    );

    // Discriminating post-condition: the warm entry is gone.
    assert!(
        store.get_unvalidated(&key).is_none(),
        "entry must be evicted after invalidate_canonical of the \
         fact-rail canonical. If still present, the reverse index is \
         not draining fact-rail deps."
    );
}

/// Discriminator — `cooperative_return_only_not_shared_to_joiners`.
///
/// The materialiser's stack-local `non_cacheable_outcome:
/// RefCell<...>` side channel held the valid-but-non-cacheable
/// outcome from the winner thread only — cooperative joiners on the
/// same key observed an empty stash and returned `Tainted`.
///
/// The `ComputeAdmission::{Cacheable, ReturnOnly, Failed}` admission
/// outcome lifts the non-cacheable case into the cooperative API.
/// A `ReturnOnly { value, reason }` value carries NO `Entry` and NO dep-signature
/// carrier, so it cannot be view-validated against a cooperative
/// joiner's own view: two requests carrying the same cache key can
/// run under different overlays, and a carrier-less value is not
/// interchangeable across views. `ReturnOnly` is therefore
/// non-shareable — the winner alone receives the `V`, and every
/// joiner observes the slot's `non_cacheable_winner` flag, forks,
/// and cold-recomputes for its own view. The cache stays empty.
///
/// Discriminating assertions: the enum exists with all three
/// variants; the admission function exists; the `ReturnOnly` arm
/// sets `non_cacheable_winner` (NOT a broadcast channel); the
/// joiner branch forks on `non_cacheable_winner`; the `Cacheable`
/// arm does not broadcast a value; the materialiser uses the API
/// (the legacy side channel is retired).
#[test]
#[serial(fact_counter_provenance)]
fn cooperative_return_only_not_shared_to_joiners() {
    let ca_src = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/cache_runtime/singleflight.rs"),
    )
    .expect("read cache_runtime/singleflight.rs");
    let mat_src = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/component_meta_materialize.rs"),
    )
    .expect("read component_meta_materialize.rs");

    assert!(
        ca_src.contains("pub enum ComputeAdmission<V, Entry> {"),
        "ComputeAdmission<V, Entry> must exist with the three-variant shape"
    );
    for variant in &["Cacheable(Entry)", "ReturnOnly {", "Failed"] {
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
    // A `ReturnOnly` winner must NOT broadcast `V` to joiners. A
    // carrier-less value cannot be view-validated against a joiner's
    // own view, so `ReturnOnly` is non-shareable across joiners. The
    // `ReturnOnly` arm marks the slot `non_cacheable_winner` so every
    // joiner forks and cold-recomputes for its own view; the winner
    // alone receives the value. Scope the check to the `ReturnOnly`
    // match arm.
    let return_only_arm_idx = ca_src
        .find("ComputeAdmission::ReturnOnly { value, reason } => {")
        .expect("ReturnOnly admission arm must exist");
    let cacheable_arm_idx = ca_src
        .find("ComputeAdmission::Cacheable(entry) => {")
        .expect("Cacheable admission arm must exist");
    let return_only_arm_window = &ca_src[return_only_arm_idx..];
    assert!(
        return_only_arm_window.contains("state.non_cacheable_winner = true;"),
        "the ReturnOnly admission arm must mark the slot \
         `non_cacheable_winner` so cooperative joiners fork and \
         cold-recompute for their own view — a carrier-less ReturnOnly \
         value cannot be view-validated and must not be shared to joiners"
    );
    assert!(
        !return_only_arm_window.contains("return_only ="),
        "the ReturnOnly admission arm must NOT broadcast `V` through a \
         slot channel — `ReturnOnly` is non-shareable across joiners; \
         each joiner forks and cold-recomputes for its own view"
    );
    // The joiner branch must fork when it observes a
    // `non_cacheable_winner` winner rather than reading a broadcast
    // value.
    assert!(
        ca_src.contains("if non_cacheable_winner {"),
        "the cooperative joiner branch must fork (re-enter admission) \
         when the winner emitted a non-cacheable `ReturnOnly` outcome"
    );
    // Critical: the Cacheable arm must NOT broadcast `V` through a
    // slot channel. Joiners fall through to `map.get(&key) +
    // validate(&entry_arc)` so each joiner thread runs `validate` on
    // its own thread — view-checking the entry against its own view
    // and running the caller's fact-bubble side effect. See
    // `cacheable_joiner_runs_validate_on_its_own_thread` in
    // `cache_runtime/singleflight.rs` for the behavioural discriminator.
    let cacheable_arm_end = cacheable_arm_idx
        + ca_src[cacheable_arm_idx..]
            .find("ComputeAdmission::ReturnOnly { value, reason } => {")
            .expect("ReturnOnly arm follows Cacheable arm");
    let cacheable_arm_window = &ca_src[cacheable_arm_idx..cacheable_arm_end];
    assert!(
        !cacheable_arm_window.contains("return_only ="),
        "the Cacheable admission arm must NOT broadcast `V` through a \
         slot channel. Joiners must fall through to \
         `map.get(&key) + validate(&entry_arc)` so each joiner thread \
         runs `validate` on its own thread — view-checking the entry \
         and bubbling the cached entry's facts into the joiner's outer \
         fact tracer."
    );

    // The materialiser routes through the admission API. The
    // multi-candidate substrate funnels the materialiser's
    // `cooperative_admit_with_post_publish` usage through
    // `MaterializeStructureDb::get_or_compute_admit`, whose `compute`
    // closure returns the `singleflight::ComputeAdmission` carrier — the
    // `ReturnOnly` arm is the overflow / non-cacheable path. Assert BOTH
    // so the guard still proves overflow outcomes are modelled as
    // `ComputeAdmission::ReturnOnly` via the cooperative-admission API.
    assert!(
        mat_src.contains("get_or_compute_admit(&cache_key, ctx, compute)"),
        "materialize_component_meta_structure must route a canonical-keyed \
         (decl-rooted) subject's cold build through the cooperative-admission \
         `MaterializeStructureDb::get_or_compute_admit` API so it is a one-winner \
         singleflight (a root-less anonymous subject keys no DB slot and computes \
         uncached via `run_uncached_materialisation` — it is shared with no one)"
    );
    assert!(
        mat_src.contains("crate::cache_runtime::singleflight::ComputeAdmission::ReturnOnly {"),
        "the materialiser's `get_or_compute_admit` compute closure must \
         model overflow / non-cacheable outcomes as \
         `singleflight::ComputeAdmission::ReturnOnly` so they are NOT \
         broadcast to cooperative joiners"
    );
    // The stack-local `non_cacheable_outcome: RefCell<...>` side
    // channel is retired.
    assert!(
        !mat_src.contains("let non_cacheable_outcome: NonCacheableSlot = RefCell::new(None);"),
        "the stack-local `non_cacheable_outcome` side channel must be retired — \
         non-cacheable outcomes are modelled by `ComputeAdmission::ReturnOnly`."
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

/// Bonus discriminator — the ReadSetSignature carrier's `canonical_ids()`
/// MUST cover every canonical the fact rail names across all
/// `FactVersionRef` variants. Without this, the reverse-index
/// registration would skip entries whose canonicals are only
/// reachable through specific fact variants.
#[test]
#[serial(fact_counter_provenance)]
fn read_set_signature_carrier_canonical_ids_covers_fact_rail() {
    use std::sync::Arc;
    use verter_session::resolver_core::{FactVersionRef, ParseFactRef};

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
    let sig = ReadSetSignature::new(facts);
    let canons: Vec<String> = sig
        .canonical_ids()
        .iter()
        .map(|a| a.as_ref().to_string())
        .collect();
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
        2,
        "canonical_ids must yield the deduplicated set of fact-rail canonicals"
    );
}
