//! R7 cross-owner `MaterializeStructureDb` reuse test (Stage 5
//! Sub-task D).
//!
//! Per plan §"Stage 5 / Sub-task D" + plan §"Verify" bullet 2:
//! "Discriminating cross-owner reuse test on `MaterializeStructureDb`:
//! pre-Stage-0 = N entries; post-Stage-5 = 1 entry."
//!
//! **Discriminating contract:** the cache key's `Hash`/`PartialEq`
//! must EXCLUDE `scope_canonical_id`. N concurrent materialise
//! requests for the same `(base, scope_axis, mode)` reached from
//! distinct consumer scopes must land in ONE cache entry, not N.
//!
//! Pre-Stage-5d (legacy `#[derive(Hash, PartialEq)]` including
//! `scope_canonical_id`): N entries.
//! Post-Stage-5d (hand-rolled `Hash`/`PartialEq` excluding
//! `scope_canonical_id`): 1 entry.

use std::collections::HashSet;
use std::sync::Arc;

use verter_session::component_meta_materialize::{
    MaterializationScope, MaterializeStructureCacheKey,
};
use verter_session::semantic_query::{ProjectionMode, SemanticNodeId};

fn key_for(scope: &str, base: SemanticNodeId) -> MaterializeStructureCacheKey {
    MaterializeStructureCacheKey {
        scope_canonical_id: Arc::from(scope),
        base,
        scope_axis: MaterializationScope::TopLevel,
        mode: ProjectionMode::Expanded,
    }
}

/// R7 — cross-owner reuse: N consumer scopes reaching the SAME
/// `(base, scope_axis, mode)` produce ONE cache slot. Verified
/// via key equality: keys constructed with different
/// `scope_canonical_id` must compare equal and hash to the same
/// bucket.
#[test]
fn r7_cross_owner_keys_compare_equal_when_only_scope_differs() {
    // SemanticNodeId is a transparent newtype around u32 with
    // Default::default returning the synthetic-id NULL value;
    // use a stable id for the test.
    let base = SemanticNodeId(0);

    let key_a = key_for("/src/ConsumerA.vue", base);
    let key_b = key_for("/src/ConsumerB.vue", base);
    let key_c = key_for("/src/ConsumerC.vue", base);
    let key_d = key_for("/src/ConsumerD.vue", base);

    assert_eq!(key_a, key_b);
    assert_eq!(key_a, key_c);
    assert_eq!(key_a, key_d);

    // Hash equality follows from PartialEq equality (HashSet
    // contract): N distinct consumer scopes collapse to ONE bucket.
    let mut set: HashSet<MaterializeStructureCacheKey> = HashSet::new();
    set.insert(key_a);
    set.insert(key_b);
    set.insert(key_c);
    set.insert(key_d);

    assert_eq!(
        set.len(),
        1,
        "N consumer scopes for the same (base, scope_axis, mode) must collapse to 1 cache slot"
    );
}

/// R7 — discrimination: keys with DIFFERENT `base` still produce
/// distinct entries even if `scope_canonical_id` matches.
#[test]
fn r7_distinct_bases_produce_distinct_entries() {
    let scope = "/src/Consumer.vue";
    let base_1 = SemanticNodeId(0);
    let base_2 = SemanticNodeId(42);
    assert_ne!(base_1, base_2);

    let key_1 = key_for(scope, base_1);
    let key_2 = key_for(scope, base_2);

    assert_ne!(
        key_1, key_2,
        "distinct base nodes must produce distinct cache keys"
    );

    let mut set: HashSet<MaterializeStructureCacheKey> = HashSet::new();
    set.insert(key_1);
    set.insert(key_2);
    assert_eq!(set.len(), 2);
}

/// R7 — discrimination: keys with different `scope_axis` produce
/// distinct entries.
#[test]
fn r7_distinct_scope_axis_produces_distinct_entries() {
    let base = SemanticNodeId(0);
    let scope = "/src/Consumer.vue";

    let key_toplevel = MaterializeStructureCacheKey {
        scope_canonical_id: Arc::from(scope),
        base,
        scope_axis: MaterializationScope::TopLevel,
        mode: ProjectionMode::Expanded,
    };
    let key_nested = MaterializeStructureCacheKey {
        scope_canonical_id: Arc::from(scope),
        base,
        scope_axis: MaterializationScope::Nested,
        mode: ProjectionMode::Expanded,
    };

    assert_ne!(key_toplevel, key_nested);
}

/// R7 — discrimination: keys with different `mode` produce distinct
/// entries.
#[test]
fn r7_distinct_projection_mode_produces_distinct_entries() {
    let base = SemanticNodeId(0);
    let scope = "/src/Consumer.vue";

    let key_expanded = MaterializeStructureCacheKey {
        scope_canonical_id: Arc::from(scope),
        base,
        scope_axis: MaterializationScope::TopLevel,
        mode: ProjectionMode::Expanded,
    };
    let key_navigate = MaterializeStructureCacheKey {
        scope_canonical_id: Arc::from(scope),
        base,
        scope_axis: MaterializationScope::TopLevel,
        mode: ProjectionMode::Navigate,
    };

    assert_ne!(key_expanded, key_navigate);
}

/// R7 — Stage 5d end-state placeholder type. The richer
/// `MaterializationCacheKey` is introduced alongside the legacy
/// key for Stage 6+ migration. This test pins the structural
/// shape (5 fields).
#[test]
fn r7_materialization_cache_key_has_5_fields() {
    use verter_session::component_meta_materialize::{
        MaterializationCacheKey, ProjectionPathHash, TypeArgsHash,
    };
    use verter_session::semantic_query::{
        HashValue, ResolvedDeclSlotIdentity, SemanticSymbolSpace,
    };

    let slot = ResolvedDeclSlotIdentity::type_slot(
        Arc::from("/src/ChatMessageProps.ts"),
        Arc::from("ChatMessageProps"),
        1,
        [1; 16],
        [2; 16],
    );
    let key = MaterializationCacheKey {
        decl: slot.clone(),
        projection_path: ProjectionPathHash([3; 16]),
        projection_mode: ProjectionMode::Expanded,
        normalized_type_args: TypeArgsHash([4; 16]),
        options_hash: HashValue::default(),
    };

    // The five fields are accessible.
    assert_eq!(key.decl.merged_symbol_name.as_ref(), "ChatMessageProps");
    assert_eq!(key.decl.symbol_space, SemanticSymbolSpace::Type);
    assert_eq!(key.projection_path.0, [3; 16]);
    assert_eq!(key.projection_mode, ProjectionMode::Expanded);
    assert_eq!(key.normalized_type_args.0, [4; 16]);
    assert_eq!(key.options_hash, HashValue::default());

    // The key hashes deterministically.
    let mut s1: HashSet<MaterializationCacheKey> = HashSet::new();
    s1.insert(key.clone());
    s1.insert(key.clone());
    assert_eq!(s1.len(), 1);
}
