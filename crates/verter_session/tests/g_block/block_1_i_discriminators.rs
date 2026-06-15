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
fn execute_read_cold_build_persists_traced_facts() {
    let _serial = DISCRIMINATOR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());

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
        mod_src.contains("install_fact_tracer(host"),
        "shared cold-build helper must wrap the cold-build closure with install_fact_tracer"
    );
    assert!(
        raise_src.contains("self.execute_via_cold_build_helper(key)"),
        "execute_read must delegate to the shared cold-build helper so its cold builds \
         install the fact tracer"
    );
    assert!(
        !raise_src.contains("graph.execute_cooperative("),
        "execute_read must NOT call graph.execute_cooperative directly. A separate \
         `graph.execute_cooperative(...)` call inside execute_read \
         would bypass install_fact_tracer."
    );
}

/// Discriminator — `semantic_memo_fact_only_invalidation_drops_slot`.
///
/// A tree whose `invalidate_canonical` walks only the
/// reverse index registered from the legacy `DepSignature`
/// canonicals. A memo entry whose `read_set_signature` referenced
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

    // The memo's reverse-index registration must drive its
    // canonical iteration from the entry's full carrier (the union
    // of legacy + facts canonicals), not the legacy `DepSignature`
    // alone. Without this, a fact-only canonical that the legacy
    // signature does not name has no shard for
    // `invalidate_canonical` to drain — leaving the memo entry
    // orphaned across invalidation. The registration helper lives in
    // `semantic_query_memo::reverse_index`; the family memo
    // (`mod.rs`) routes its publish paths through
    // `reverse_index::register_reverse_index(...)`.
    let reverse_index_src = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/semantic_query_memo/reverse_index.rs"),
    )
    .expect("read reverse_index.rs");
    let register_idx = reverse_index_src
        .find("fn register_reverse_index(")
        .expect("reverse_index.rs must declare register_reverse_index");
    let register_window = &reverse_index_src
        [register_idx..register_idx + 4000.min(reverse_index_src.len() - register_idx)];
    assert!(
        register_window.contains("read_set_signature.canonical_ids()"),
        "register_reverse_index must iterate \
         `read_set_signature.canonical_ids()` so the memo's reverse \
         index drains under every canonical the entry's carrier \
         references — including fact-only canonicals (Parse / \
         ResolveImports / RouteSurface). A registration keyed only on \
         the legacy `dep_signature` rail would lose fact-only \
         invalidation. See the behavioural test \
         `semantic_memo_invalidate_drains_fact_canonical_entry`."
    );
    // The family memo must invoke the relocated helper on its publish
    // paths — without the call sites the registration helper is dead and
    // fact-only invalidation never drives reverse-index registration.
    assert!(
        memo_mod_src.contains("reverse_index::register_reverse_index("),
        "the family memo (`mod.rs`) must route its publish paths through \
         `reverse_index::register_reverse_index(...)` so every published \
         entry registers under each canonical its carrier names."
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
fn semantic_memo_invalidate_drains_fact_canonical_entry() {
    let _serial = DISCRIMINATOR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());

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
            local_scope: None,
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

/// Discriminator — `semantic_memo_warm_hit_validates_before_bubble`.
///
/// A tree whose `SemanticGraphStore::get` and
/// `try_warm_hit_fast_path` bubble `entry.read_set_signature`
/// unconditionally has a hole: a stale entry (carrier no longer validates
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
    let family_src = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/semantic_query_memo/family.rs"),
    )
    .expect("read family.rs");
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
    // Under the multi-candidate `FamilySlots` substrate, the
    // validate-before-bubble gate lives inside `FamilySlots::lookup`:
    // the candidate-list scan calls `entry.validate(ctx)` for each
    // candidate and returns the first that validates. The strict
    // self-root rail is `ReadSetSignature::validate_with_self_roots`
    // — `MemoEntry::validate` is the single-line forwarder. Stale
    // candidates are SKIPPED without bubbling.
    assert!(
        family_src.contains(".validate(ctx)") && family_src.contains("validate_with_self_roots"),
        "FamilySlots::lookup must call `entry.validate(ctx)` for each \
         candidate BEFORE returning it (validate-before-bubble gate). \
         `MemoEntry::validate` routes through the strict self-root validator \
         `ReadSetSignature::validate_with_self_roots`."
    );
    assert!(
        build_src.contains("graph.get_validated(&prefix_key, ctx)"),
        "find_longest_warm_prefix must consult `get_validated` so the \
         prefix-probe never returns a stale entry's facts."
    );
    assert!(
        !build_src.contains("graph.get(&prefix_key)")
            && !build_src.contains("graph.get_unvalidated(&prefix_key)"),
        "find_longest_warm_prefix must NOT use the unchecked `graph.get(...)` / `graph.get_unvalidated(...)` — the bubble-without-validate is the stale-entry hole."
    );
}

/// Discriminator — `materialize_structure_peek_and_register_use_carrier`.
///
/// A tree whose `MaterializeStructureDb::peek` validates only
/// the legacy `dep_signature` rail has a hole: an entry whose facts rail
/// referenced a stale fact (e.g. a `Parse(MemberPresence(Foo, a))`
/// fact for a member that no longer exists) would survive the peek
/// even though the path-precise observation was stale.
///
/// The carrier-aware peek calls
/// `entry.read_set_signature.validate_with_self_roots(ctx, ...)` which
/// AND-gates both rails AND validates the entry's self-root canonicals
/// (ONLY the `base` node's declaration-origin file — the consumer
/// materialise scope is NOT a self-root, R7 cross-owner reuse)
/// **strictly** — a strictly stronger gate than the plain
/// `validate(ctx)` the pre-self-root tree used.
/// `register_post_publish` keys the reverse index under the carrier's
/// `canonical_ids()` (union of legacy + facts canonicals).
///
/// Discriminating assertions: peek strict-validates via the carrier
/// and register_post_publish uses the carrier.
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
    // Under the multi-candidate substrate the peek lookup closure
    // receives each `candidate` from the shared
    // `ReverseIndexedCandidateStore`; the carrier is the candidate's
    // `signature` field and the strict gate is
    // `validate_with_self_roots(ctx, &candidate.self_root_canonicals)`.
    assert!(
        peek_window.contains("candidate")
            && peek_window.contains(".signature")
            && peek_window
                .contains("validate_with_self_roots(ctx, &candidate.self_root_canonicals)"),
        "MaterializeStructureDb::peek must AND-gate each candidate via the carrier's strict \
         `candidate.signature.validate_with_self_roots(ctx, &candidate.self_root_canonicals)` so \
         the carrier's facts rail invalidates a stale entry even when the legacy DepSignature \
         still validates, AND a same-canonical edit to a self-root (ONLY the `base` node's \
         declaration-origin file — NOT the consumer materialise scope) rejects the candidate \
         strictly. Pre-fix peek validated only the legacy rail; the pre-self-root tree \
         used the lax `validate`."
    );

    // The post-publish reverse-index registration relocated into the
    // shared `ReverseIndexedCandidateStore`: `publish_core` drives its
    // per-canonical registration from `reverse_index_canonicals`, which
    // keys the index under the UNION of the carrier's
    // `signature.canonical_ids()` and the candidate's strict
    // `self_root_canonicals`. The carrier is the candidate's `signature`
    // (`ReadSetSignature`) field. Assert against the relocated path so the
    // guard still proves the reverse index drains every canonical the
    // carrier references (including fact-only deps).
    let candidate_store_src = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/cache_runtime/candidate_store.rs"),
    )
    .expect("read cache_runtime/candidate_store.rs");
    assert!(
        candidate_store_src.contains("fn reverse_index_canonicals<V>(")
            && candidate_store_src.contains("candidate: &Candidate<FactCandidateDiscriminant, V>"),
        "the shared store's `reverse_index_canonicals` must accept the carrier-bearing \
         `Candidate` so the reverse index drains every canonical the carrier references \
         (union of the facts' canonicals + the strict self-roots). Pre-fix the reverse \
         index was keyed only by legacy DepSignature canonicals."
    );
    assert!(
        candidate_store_src.contains("candidate.signature.canonical_ids()"),
        "reverse_index_canonicals must iterate the carrier's \
         `candidate.signature.canonical_ids()` so fact-only deps register the candidate \
         under the changed canonical's reverse-index slot."
    );
    assert!(
        candidate_store_src.contains("reverse_index_canonicals(&candidate)"),
        "publish_core must drive its per-canonical reverse-index registration from \
         `reverse_index_canonicals(&candidate)` so every published candidate is \
         registered under each canonical its carrier names."
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
/// A `ReturnOnly(V)` value carries NO `Entry` and NO dep-signature
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
fn cooperative_return_only_not_shared_to_joiners() {
    let _serial = DISCRIMINATOR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());

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
    // A `ReturnOnly` winner must NOT broadcast `V` to joiners. A
    // carrier-less value cannot be view-validated against a joiner's
    // own view, so `ReturnOnly` is non-shareable across joiners. The
    // `ReturnOnly` arm marks the slot `non_cacheable_winner` so every
    // joiner forks and cold-recomputes for its own view; the winner
    // alone receives the value. Scope the check to the `ReturnOnly`
    // match arm.
    let return_only_arm_idx = ca_src
        .find("ComputeAdmission::ReturnOnly(value) => {")
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
            .find("ComputeAdmission::ReturnOnly(value) => {")
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
        mat_src.contains("crate::cache_runtime::singleflight::ComputeAdmission::ReturnOnly("),
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

/// Bonus discriminator — `ComputeAdmission::Failed` must be
/// constructible. The three-variant contract requires a
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
            verter_session::for_tests::ComputeAdmission::Failed
        ),
        "ComputeAdmission::Failed must be constructible — the three-variant \
         contract requires Cacheable / ReturnOnly / Failed."
    );
}

/// Bonus discriminator — the ReadSetSignature carrier's `canonical_ids()`
/// MUST cover every canonical the fact rail names across all
/// `FactVersionRef` variants. Without this, the reverse-index
/// registration would skip entries whose canonicals are only
/// reachable through specific fact variants.
#[test]
fn read_set_signature_carrier_canonical_ids_covers_fact_rail() {
    let _serial = DISCRIMINATOR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());

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
