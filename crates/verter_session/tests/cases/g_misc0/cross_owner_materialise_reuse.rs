//! R7 cross-owner `MaterializeStructureDb` reuse — key-shape unit tests.
//!
//! Two layers carry the cross-owner-reuse property:
//!
//! - The per-thread recursion/depth identity [`MaterializeRuntimeKey`]
//!   excludes `scope_canonical_id` from `Hash`/`PartialEq` (the
//!   recursion identity does not depend on which consumer reached the
//!   node).
//! - The DB cache key [`MaterializationCacheKey`] is content-free and
//!   carries NO consumer-scope dimension at all — so N consumer scopes
//!   reaching the same canonical subject structurally collapse onto ONE
//!   slot. (The end-to-end / dispatch-level entry-count discrimination
//!   lives in `cross_owner_materialise_reuse_production.rs`.)

use std::collections::HashSet;
use std::sync::Arc;

use verter_session::component_meta_materialize::{MaterializationScope, MaterializeRuntimeKey};
use verter_session::semantic_query::{ProjectionMode, SemanticNodeId};

fn key_for(scope: &str, base: SemanticNodeId) -> MaterializeRuntimeKey {
    MaterializeRuntimeKey {
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
    let mut set: HashSet<MaterializeRuntimeKey> = HashSet::new();
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

    let mut set: HashSet<MaterializeRuntimeKey> = HashSet::new();
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

    let key_toplevel = MaterializeRuntimeKey {
        scope_canonical_id: Arc::from(scope),
        base,
        scope_axis: MaterializationScope::TopLevel,
        mode: ProjectionMode::Expanded,
    };
    let key_nested = MaterializeRuntimeKey {
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

    let key_expanded = MaterializeRuntimeKey {
        scope_canonical_id: Arc::from(scope),
        base,
        scope_axis: MaterializationScope::TopLevel,
        mode: ProjectionMode::Expanded,
    };
    let key_navigate = MaterializeRuntimeKey {
        scope_canonical_id: Arc::from(scope),
        base,
        scope_axis: MaterializationScope::TopLevel,
        mode: ProjectionMode::Navigate,
    };

    assert_ne!(key_expanded, key_navigate);
}

/// R6/R21 — the content-free canonical-subject `MaterializationCacheKey`
/// is the DB cache key. This pins its field set (canonical subject slot +
/// typed projection path + policy axis + mode + instantiation args +
/// resolve_env_hash) and proves it carries NO consumer-scope dimension
/// (cross-owner reuse is structural) and NO content/version hash (R6).
#[test]
fn r7_materialization_cache_key_is_content_free_canonical_subject() {
    use verter_session::component_meta_materialize::MaterializationCacheKey;
    use verter_session::resolver_core::RouteDemand;
    use verter_session::semantic_query::{ResolvedDeclSlotIdentity, SemanticSymbolSpace};

    let slot = ResolvedDeclSlotIdentity::type_slot(
        Arc::from("/src/ChatMessageProps.ts"),
        Arc::from("ChatMessageProps"),
        1,
        [1; 16],
        [2; 16],
    );
    let key = MaterializationCacheKey {
        decl: slot.clone(),
        projection_path: RouteDemand::pick(vec!["id".to_string()]),
        scope_axis: MaterializationScope::TopLevel,
        projection_mode: ProjectionMode::Expanded,
        normalized_type_args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        resolve_env_hash: [9; 16],
    };

    // The canonical subject + axes are accessible; the subject is the
    // env-bearing slot (carrying project_identity / type_env / lib_env),
    // never a graph-instance SemanticNodeId.
    assert_eq!(key.decl.merged_symbol_name.as_ref(), "ChatMessageProps");
    assert_eq!(key.decl.symbol_space, SemanticSymbolSpace::Type);
    assert_eq!(
        key.projection_path,
        RouteDemand::pick(vec!["id".to_string()])
    );
    assert_eq!(key.scope_axis, MaterializationScope::TopLevel);
    assert_eq!(key.projection_mode, ProjectionMode::Expanded);
    assert_eq!(key.resolve_env_hash, [9; 16]);

    // The key hashes deterministically and is Eq.
    let mut s1: HashSet<MaterializationCacheKey> = HashSet::new();
    s1.insert(key.clone());
    s1.insert(key.clone());
    assert_eq!(s1.len(), 1);

    // resolve_env_hash discriminates the slot (R21 split-env): the same
    // subject under a different resolve env is a DISTINCT entry.
    let mut env_variant = key.clone();
    env_variant.resolve_env_hash = [7; 16];
    assert_ne!(
        hash_of(&key),
        hash_of(&env_variant),
        "resolve_env_hash must distinguish the MaterializationCacheKey slot",
    );

    // A different projection (Pick<'id'> vs Pick<'body'>) is a DISTINCT
    // entry — path-precise, no over-share across projections.
    let mut proj_variant = key.clone();
    proj_variant.projection_path = RouteDemand::pick(vec!["body".to_string()]);
    assert_ne!(
        hash_of(&key),
        hash_of(&proj_variant),
        "distinct projection paths must not alias onto one slot",
    );
}

fn hash_of<T: std::hash::Hash>(value: &T) -> u64 {
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}
