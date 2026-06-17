//! Self-version-root discriminators for the `SemanticGraphStore`
//! query-node memo.
//!
//! Every `SemanticGraphStore` query-node memo entry is
//! self-version-rooted: its [`crate::fact_signature_helpers::ReadSetSignature`]
//! carrier leads with a `FileWholeHash` fact for each canonical the
//! cold build's value depends on for its own identity (the keyed
//! canonical for `ResolveDecl` / `TypeOf` / `Instantiate` /
//! `ResolveMacroPayload`, or the file-derived origin of every input
//! node for the node kinds keyed by interned `SemanticNodeId`s). The
//! warm-read validator (`get_validated`, the `execute_cooperative`
//! fast path, the relation memo) validates those self-roots strictly:
//! a same-canonical content edit, or a self-root canonical the live
//! store view no longer tracks, rejects the entry and forces a
//! recompute.
//!
//! ## Discrimination model
//!
//! Each `*_same_canonical_edit_*` test below:
//!
//! 1. Loads a `.ts`/`.vue` fixture and primes a warm query-node memo
//!    entry by dispatching the relevant `SemanticQueryKey`.
//! 2. Edits the entry's defining canonical through the production
//!    [`crate::VerterHost::upsert`]. The upsert performs no
//!    own-canonical query-identity cache drain, so the warm graph entry
//!    physically survives. The test therefore observes whether the
//!    entry detects the same-canonical edit on its own self-root.
//! 3. Asserts the warm read (`get_validated`) MISSES — the strict
//!    self-root validator rejects the entry because the file's whole
//!    hash shifted — while `get_unvalidated` still finds the (now
//!    stale) physical entry.
//!
//! Discriminating property: without strict self-root validation, the
//! `SemanticGraphStore` query nodes would return a stale entry on a
//! warm read after a same-canonical edit. With strict self-root
//! validation the warm read misses. Each test's
//! `get_unvalidated`-still-`Some` assertion proves the entry was
//! physically present (the discrimination is the validator rejecting
//! it, not the entry being absent).

use std::sync::Arc;

use verter_semantic::facts::{FactKey, FactLane};

use crate::fact_signature_helpers::ReadSetSignature;
use crate::resolver_core::{FactReadSetFinalise, FactVersionRef, ResolverContext};
use crate::semantic_query::{
    DepSignature, ResolveDeclKey, ScopeId, SemanticNodeData, SemanticNodeId, SemanticQueryApi,
    SemanticQueryKey,
};
use crate::semantic_query_memo::{semantic_graph_read_set_signature, SemanticGraphStore};
use crate::{HostConfig, UpsertRequest, VerterHost};

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

/// Upsert through the production [`VerterHost::upsert`] path. The
/// upsert performs no own-canonical query-identity cache drain, so a
/// warm graph entry for the upserted canonical physically survives and
/// the test can observe whether its self-root validation detects the
/// edit.
fn upsert(host: &VerterHost, path: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(path.to_string()),
            input_id: path.to_string(),
            source: Arc::from(source),
            file_language: crate::LanguageRegistry::global()
                .classify_static(path)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .expect("upsert succeeds");
}

fn resolve_decl_key(canonical: &str, name: &str) -> ResolveDeclKey {
    ResolveDeclKey {
        scope: ScopeId {
            canonical_id: Arc::from(canonical),
            local_scope: None,
        },
        name: Arc::from(name),
    }
}

// ---------------------------------------------------------------------------
// Per-node-kind same-canonical-edit discriminators (end-to-end).
// ---------------------------------------------------------------------------

/// `ResolveDecl` — a same-canonical content edit rejects the warm
/// query-node memo entry.
///
/// Discriminating property: the `ResolveDecl` memo entry self-roots on
/// `scope.canonical_id`'s `FileWholeHash`. After the same-canonical
/// edit the file's whole hash shifts; the strict self-root validator
/// rejects the warm entry. Without strict self-root validation the
/// entry would not be strictly self-rooted and the warm read would
/// return it stale.
#[test]
fn resolve_decl_same_canonical_edit_rejects_warm_entry() {
    let host = host();
    let c = "/sg_self_root/resolve_decl.ts";
    upsert(&host, c, "export type Foo = { a: number };\n");
    let dispatch = host.semantic_dispatch();
    let key = SemanticQueryKey::ResolveDecl(resolve_decl_key(c, "Foo"));

    // Prime the warm memo entry.
    let primed = dispatch.execute_type_node(key.clone());
    assert!(
        matches!(primed, crate::semantic_query::QueryResult::Value(_)),
        "ResolveDecl must resolve before the edit"
    );
    let graph = host.project_type_store().semantic_graph();
    assert!(
        graph.get_unvalidated(&key).is_some(),
        "fixture invariant: the warm memo entry must exist after priming"
    );

    // Edit the keyed canonical: the upsert performs no own-canonical
    // drain, so the entry survives and the warm read below exercises
    // the entry's self-root validation directly.
    upsert(&host, c, "export type Foo = { a: string; b: number };\n");

    let ctx: &dyn ResolverContext = &host;
    assert!(
        graph.get_unvalidated(&key).is_some(),
        "the physical memo entry must still be present after the same-canonical edit \
         — the discrimination is the validator rejecting it, not its absence"
    );
    assert!(
        graph.get_validated(&key, ctx).is_none(),
        "ResolveDecl: a same-canonical content edit MUST reject the warm memo entry \
         via strict self-root validation — the entry self-roots on the keyed \
         canonical's FileWholeHash and the file's whole hash shifted",
    );
}

/// `TypeOf` — a same-canonical content edit rejects the warm
/// query-node memo entry.
///
/// Discriminating property: identical to the `ResolveDecl` case — the
/// `TypeOf` memo entry self-roots on `value_root.scope.canonical_id`.
#[test]
fn typeof_same_canonical_edit_rejects_warm_entry() {
    let host = host();
    let c = "/sg_self_root/typeof.ts";
    upsert(&host, c, "export const val = { a: 1 };\n");
    let dispatch = host.semantic_dispatch();
    let key = dispatch.typeof_key_for(
        crate::semantic_query::ValueRootKey {
            scope: ScopeId {
                canonical_id: Arc::from(c),
                local_scope: None,
            },
            name: Arc::from("val"),
        },
        crate::semantic_query::ProjectionReductionContext::published(
            crate::semantic_query::ProjectionMode::Expanded,
        ),
    );

    let primed = dispatch.execute_type_node(key.clone());
    assert!(
        matches!(primed, crate::semantic_query::QueryResult::Value(_)),
        "TypeOf must resolve before the edit"
    );
    let graph = host.project_type_store().semantic_graph();
    // Hard fixture invariant: `TypeOf` of a value with an object
    // initializer MUST warm a memo entry. An early `return` here would
    // make the test pass vacuously if the `TypeOf` kind ever stopped
    // publishing — the discrimination is the validator rejecting the
    // entry, not its absence.
    assert!(
        graph.get_unvalidated(&key).is_some(),
        "fixture invariant: the warm TypeOf memo entry must exist after priming"
    );

    upsert(&host, c, "export const val = { a: 1, b: 2 };\n");

    let ctx: &dyn ResolverContext = &host;
    assert!(
        graph.get_unvalidated(&key).is_some(),
        "the physical TypeOf memo entry must still be present after the same-canonical edit"
    );
    assert!(
        graph.get_validated(&key, ctx).is_none(),
        "TypeOf: a same-canonical content edit MUST reject the warm memo entry \
         via strict self-root validation",
    );
}

/// `Instantiate` — a same-canonical content edit to the declaring file
/// rejects the warm query-node memo entry.
///
/// Discriminating property: the `Instantiate` memo entry self-roots on
/// the declaring canonical (`DeclIdentity.canonical_id`), threaded from
/// the observed `whole_hash`. Editing the declaring file shifts its
/// whole hash and the strict self-root validator rejects the entry.
#[test]
fn instantiate_same_canonical_edit_rejects_warm_entry() {
    let host = host();
    let c = "/sg_self_root/instantiate.ts";
    upsert(&host, c, "export type Box<T> = { value: T };\n");
    let dispatch = host.semantic_dispatch();
    let graph = host.project_type_store().semantic_graph();

    // Resolve the declaration so the observed whole hash is reachable.
    let decl_key = SemanticQueryKey::ResolveDecl(resolve_decl_key(c, "Box"));
    let _ = dispatch.execute_type_node(decl_key);
    let _ = host
        .ensure_indexed_ready(c)
        .map(|indexed| indexed.whole_hash)
        .expect("declaring file IndexedReady materialises");
    let string_arg = graph.intern_node(SemanticNodeData::Primitive(
        crate::semantic_query::PrimitiveKind::String,
    ));
    let key = SemanticQueryKey::Instantiate {
        base: crate::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
            Arc::from(c),
            Arc::from("Box"),
        ),
        args: Arc::from(vec![string_arg].into_boxed_slice()),
        context: crate::semantic_query::InstantiateContext::new(
            crate::semantic_query::ProjectionReductionContext::published(
                crate::semantic_query::ProjectionMode::Expanded,
            ),
            Default::default(),
        ),
    };

    let primed = dispatch.execute_type_node(key.clone());
    assert!(
        matches!(primed, crate::semantic_query::QueryResult::Value(_)),
        "Instantiate must resolve before the edit"
    );
    assert!(
        graph.get_unvalidated(&key).is_some(),
        "fixture invariant: the warm Instantiate memo entry must exist after priming"
    );

    // Edit the declaring file.
    upsert(
        &host,
        c,
        "export type Box<T> = { value: T; tag: string };\n",
    );

    let ctx: &dyn ResolverContext = &host;
    assert!(
        graph.get_unvalidated(&key).is_some(),
        "the physical Instantiate memo entry must still be present after the same-canonical edit"
    );
    assert!(
        graph.get_validated(&key, ctx).is_none(),
        "Instantiate: a content edit to the declaring file MUST reject the warm memo \
         entry via strict self-root validation",
    );
}

/// `ResolveMacroPayload` — a same-canonical content edit to the owning
/// SFC rejects the warm query-node memo entry.
///
/// Discriminating property: the `ResolveMacroPayload` memo entry
/// self-roots on the owner SFC's `FileWholeHash`. Editing the SFC
/// shifts its whole hash and the strict self-root validator rejects
/// the entry.
#[test]
fn resolve_macro_payload_same_canonical_edit_rejects_warm_entry() {
    let host = host();
    let c = "/sg_self_root/macro.vue";
    upsert(
        &host,
        c,
        "<script setup lang=\"ts\">defineProps<{ x: string }>()</script>\n",
    );
    let dispatch = host.semantic_dispatch();
    let graph = host.project_type_store().semantic_graph();

    let _ = host
        .ensure_indexed_ready(c)
        .map(|indexed| indexed.whole_hash)
        .expect("owner SFC IndexedReady materialises");
    let arg = graph.intern_node(SemanticNodeData::Primitive(
        crate::semantic_query::PrimitiveKind::String,
    ));
    let key = SemanticQueryKey::ResolveMacroPayload {
        owner: crate::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
            Arc::from(c),
            Arc::from("<sfc-script-setup>"),
        ),
        macro_index: 0,
        macro_kind: verter_semantic::analysis::AnalyzedMacroKind::DefineProps,
        type_args: Arc::from(vec![arg].into_boxed_slice()),
        context: crate::semantic_query::MacroPayloadContext::new(
            Default::default(),
            crate::semantic_query::ProjectionMode::Expanded,
        ),
    };

    let primed = dispatch.execute_type_node(key.clone());
    assert!(
        matches!(primed, crate::semantic_query::QueryResult::Value(_)),
        "ResolveMacroPayload must resolve before the edit"
    );
    assert!(
        graph.get_unvalidated(&key).is_some(),
        "fixture invariant: the warm ResolveMacroPayload memo entry must exist after priming"
    );

    // Edit the owning SFC.
    upsert(
        &host,
        c,
        "<script setup lang=\"ts\">defineProps<{ x: string; y: number }>()</script>\n",
    );

    let ctx: &dyn ResolverContext = &host;
    assert!(
        graph.get_unvalidated(&key).is_some(),
        "the physical ResolveMacroPayload memo entry must still be present after the edit"
    );
    assert!(
        graph.get_validated(&key, ctx).is_none(),
        "ResolveMacroPayload: a content edit to the owning SFC MUST reject the warm memo \
         entry via strict self-root validation",
    );
}

// ---------------------------------------------------------------------------
// `semantic_graph_read_set_signature` producer unit tests.
// ---------------------------------------------------------------------------

/// The producer leads the facts rail with one self-root `FileWholeHash`
/// per observed self-root, pinned to the OBSERVED hash.
#[test]
fn read_set_signature_prepends_observed_self_roots() {
    let observed: Vec<(Arc<str>, [u8; 16])> = vec![(Arc::from("/w/a.ts"), [0x11; 16])];
    let carrier = semantic_graph_read_set_signature(&observed, &[])
        .expect("a single observed self-root builds a carrier");
    assert!(
        carrier.facts.iter().any(|f| matches!(
            f,
            FactVersionRef::FileWholeHash { canonical_id, hash }
                if canonical_id == "/w/a.ts" && *hash == [0x11; 16]
        )),
        "the facts rail must lead with a self-root FileWholeHash pinned to the observed hash",
    );
}

/// A conflicting observed self-root hash for the same canonical is a
/// torn observation — the producer returns `None` (non-cacheable).
#[test]
fn read_set_signature_rejects_conflicting_self_root_hashes() {
    let observed: Vec<(Arc<str>, [u8; 16])> = vec![
        (Arc::from("/w/a.ts"), [0x11; 16]),
        (Arc::from("/w/a.ts"), [0x22; 16]),
    ];
    let result = semantic_graph_read_set_signature(&observed, &[]);
    assert!(
        result.is_none(),
        "two observed self-roots for the same canonical with conflicting hashes is a torn \
         observation — the producer MUST return None so the entry is non-cacheable",
    );
}

/// A traced `FileWholeHash` fact for a self-root canonical that
/// disagrees with the observed self-root hash is a torn dependency
/// rail — the producer returns `None`.
#[test]
fn read_set_signature_rejects_traced_self_root_hash_mismatch() {
    let observed: Vec<(Arc<str>, [u8; 16])> = vec![(Arc::from("/w/a.ts"), [0x11; 16])];
    let traced = vec![FactVersionRef::FileWholeHash {
        canonical_id: "/w/a.ts".to_string(),
        hash: [0x99; 16],
    }];
    let result = semantic_graph_read_set_signature(&observed, &traced);
    assert!(
        result.is_none(),
        "a traced FileWholeHash for a self-root canonical that disagrees with the observed \
         self-root hash is a torn read — the producer MUST return None",
    );
}

/// The producer merges the traced cross-file fact set after the
/// self-roots — a traced `Parse` fact for a non-self-root canonical is
/// preserved verbatim.
#[test]
fn read_set_signature_merges_traced_cross_file_facts() {
    let observed: Vec<(Arc<str>, [u8; 16])> = vec![(Arc::from("/w/a.ts"), [0x11; 16])];
    let traced = vec![FactVersionRef::Parse(crate::resolver_core::ParseFactRef {
        canonical_id: "/w/dep.ts".to_string(),
        key: FactKey::SyntacticExportSet,
        lane: FactLane::Semantic,
        expected_hash: [0x44; 16],
    })];
    let carrier = semantic_graph_read_set_signature(&observed, &traced).expect("carrier builds");
    assert!(
        carrier.facts.iter().any(|f| matches!(
            f,
            FactVersionRef::Parse(p) if p.canonical_id == "/w/dep.ts"
        )),
        "a traced cross-file Parse fact must be merged into the carrier's facts rail",
    );
    assert!(
        carrier.facts.iter().any(|f| matches!(
            f,
            FactVersionRef::FileWholeHash { canonical_id, .. } if canonical_id == "/w/a.ts"
        )),
        "the self-root FileWholeHash must also be present",
    );
}

/// The producer keeps a traced `ProjectGeneration` fact on the carrier's
/// facts rail — a project-shape change still invalidates the entry.
#[test]
fn read_set_signature_keeps_traced_project_generation_fact() {
    let observed: Vec<(Arc<str>, [u8; 16])> = vec![(Arc::from("/w/a.ts"), [0x11; 16])];
    let traced = vec![FactVersionRef::ProjectGeneration { generation: 7 }];
    let carrier = semantic_graph_read_set_signature(&observed, &traced).expect("carrier builds");
    assert!(
        carrier.facts.iter().any(
            |f| matches!(f, FactVersionRef::ProjectGeneration { generation } if *generation == 7)
        ),
        "a traced ProjectGeneration fact must be merged into the carrier's facts rail so a \
         project-shape change invalidates the entry",
    );
}

// ---------------------------------------------------------------------------
// Strict warm-read validator unit tests.
// ---------------------------------------------------------------------------

/// `ReadSetSignature::validate_with_self_roots` rejects a carrier whose
/// self-root `FileWholeHash` names a canonical the live store view does
/// not track — strict self-root validation.
#[test]
fn validate_with_self_roots_rejects_untracked_self_root() {
    let host = host();
    // One unrelated tracked file so a live store view exists.
    upsert(&host, "/sg_self_root/anchor.ts", "export const z = 1;\n");
    let ctx: &dyn ResolverContext = &host;

    let untracked = "/sg_self_root/never_loaded.ts";
    let carrier = ReadSetSignature::new(Arc::from(vec![FactVersionRef::FileWholeHash {
        canonical_id: untracked.to_string(),
        hash: [0xCD; 16],
    }]));
    let self_roots: Arc<[Arc<str>]> = Arc::from(vec![Arc::<str>::from(untracked)]);
    assert!(
        !carrier.validate_with_self_roots(ctx, &self_roots),
        "validate_with_self_roots MUST reject a carrier whose self-root FileWholeHash names \
         an untracked canonical — the strict self-root rule",
    );
}

/// `ReadSetSignature::validate_with_self_roots` accepts a carrier whose
/// self-root `FileWholeHash` matches the tracked file's current whole
/// hash.
#[test]
fn validate_with_self_roots_accepts_matching_tracked_self_root() {
    let host = host();
    let c = "/sg_self_root/match.ts";
    upsert(&host, c, "export const z = 1;\n");
    let whole_hash = host
        .ensure_indexed_ready(c)
        .map(|indexed| indexed.whole_hash)
        .expect("IndexedReady materialises");
    let ctx: &dyn ResolverContext = &host;

    let carrier = ReadSetSignature::new(Arc::from(vec![FactVersionRef::FileWholeHash {
        canonical_id: c.to_string(),
        hash: whole_hash,
    }]));
    let self_roots: Arc<[Arc<str>]> = Arc::from(vec![Arc::<str>::from(c)]);
    assert!(
        carrier.validate_with_self_roots(ctx, &self_roots),
        "validate_with_self_roots MUST accept a carrier whose self-root FileWholeHash matches \
         the tracked file's current whole hash",
    );
}

/// An overflow carrier always fails `validate_with_self_roots` — an
/// overflowed entry must never warm-hit.
#[test]
fn validate_with_self_roots_rejects_overflow_carrier() {
    let host = host();
    upsert(&host, "/sg_self_root/anchor2.ts", "export const z = 1;\n");
    let ctx: &dyn ResolverContext = &host;
    let carrier = ReadSetSignature::overflow();
    let self_roots: Arc<[Arc<str>]> = Arc::from(Vec::<Arc<str>>::new());
    assert!(
        !carrier.validate_with_self_roots(ctx, &self_roots),
        "an overflow carrier must never validate — an overflowed entry must not warm-hit",
    );
}

// ---------------------------------------------------------------------------
// `execute_cooperative` / `execute_cooperative_batch` validate-before-return.
// ---------------------------------------------------------------------------

/// `execute_cooperative`'s warm-hit fast path validates the entry's
/// carrier before returning: a published entry whose self-root names an
/// untracked canonical is NOT served warm — the cold build re-runs.
#[test]
fn execute_cooperative_fast_path_validates_self_root() {
    let host = host();
    upsert(&host, "/sg_self_root/anchor3.ts", "export const z = 1;\n");
    let ctx: &dyn ResolverContext = &host;
    let store = SemanticGraphStore::new();
    let key =
        SemanticQueryKey::ResolveDecl(resolve_decl_key("/sg_self_root/never_loaded2.ts", "Probe"));

    // First call publishes an entry self-rooted on an untracked
    // canonical via an explicit `QueryBuildOutput` carrier.
    let untracked = "/sg_self_root/never_loaded2.ts";
    let stale_node = store.intern_node(SemanticNodeData::Primitive(
        crate::semantic_query::PrimitiveKind::String,
    ));
    let carrier = ReadSetSignature::new(Arc::from(vec![FactVersionRef::FileWholeHash {
        canonical_id: untracked.to_string(),
        hash: [0xAB; 16],
    }]));
    let self_roots: Arc<[Arc<str>]> = Arc::from(vec![Arc::<str>::from(untracked)]);
    store.publish_with_carrier_for_tests(
        key.clone(),
        crate::semantic_query::QueryResult::Value(stale_node),
        carrier,
        self_roots,
    );

    // The warm read through `execute_cooperative` must MISS — the
    // self-root canonical is untracked, so the cold build re-runs and
    // the recomputed node surfaces.
    let mut cold_ran = false;
    let recompute_node = store.intern_node(SemanticNodeData::Primitive(
        crate::semantic_query::PrimitiveKind::Number,
    ));
    let read = store.execute_cooperative(
        ctx,
        key.clone(),
        || {
            store.intern_node(SemanticNodeData::Opaque(
                crate::semantic_query::QueryError::Miss,
            ))
        },
        || {
            cold_ran = true;
            (
                crate::semantic_query::QueryResult::Value(recompute_node),
                Arc::from(Vec::new().into_boxed_slice()) as DepSignature,
            )
        },
    );
    assert!(
        cold_ran,
        "execute_cooperative's fast path MUST validate the entry's carrier before serving \
         it warm — an entry self-rooted on an untracked canonical must miss and re-run cold",
    );
    match read.value {
        crate::semantic_query::QueryResult::Value(node) => assert_eq!(
            node, recompute_node,
            "the recomputed node must surface, not the stale entry",
        ),
        other => panic!("expected the recomputed Value, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Relation memo strict warm-read validation.
// ---------------------------------------------------------------------------

/// The relation memo's warm read (`get_relation`) validates the stored
/// entry's self-version-rooted carrier strictly: an entry whose
/// self-root names an untracked canonical is NOT served warm.
#[test]
fn relation_memo_warm_read_validates_self_root() {
    let host = host();
    upsert(&host, "/sg_self_root/anchor4.ts", "export const z = 1;\n");
    let ctx: &dyn ResolverContext = &host;
    let store = SemanticGraphStore::new();
    let source = store.intern_node(SemanticNodeData::Primitive(
        crate::semantic_query::PrimitiveKind::String,
    ));
    let target = store.intern_node(SemanticNodeData::Primitive(
        crate::semantic_query::PrimitiveKind::String,
    ));

    // Publish a relation judgement whose carrier self-roots on an
    // untracked canonical.
    let untracked = "/sg_self_root/relation_never_loaded.ts";
    let carrier = ReadSetSignature::new(Arc::from(vec![FactVersionRef::FileWholeHash {
        canonical_id: untracked.to_string(),
        hash: [0xEF; 16],
    }]));
    let self_roots: Arc<[Arc<str>]> = Arc::from(vec![Arc::<str>::from(untracked)]);
    let key = crate::semantic_query::RelateMemoKey::assignable(
        source,
        target,
        crate::semantic_query::RelationContext::default(),
    );
    store.insert_relation(
        key.clone(),
        carrier,
        self_roots,
        crate::semantic_query::RelationResult::Assignable {
            bindings: Arc::from(Vec::new().into_boxed_slice()),
        },
        host.project_type_store().current_project_generation(),
    );

    assert!(
        store.get_relation(ctx, &key).is_none(),
        "the relation memo's warm read MUST validate the stored entry's self-version-rooted \
         carrier strictly — an entry self-rooted on an untracked canonical must miss",
    );
}

/// The relation memo serves a warm judgement when the stored entry's
/// self-version-rooted carrier validates (empty self-roots → vacuous).
#[test]
fn relation_memo_warm_read_serves_validated_entry() {
    let host = host();
    let ctx: &dyn ResolverContext = &host;
    let store = SemanticGraphStore::new();
    let source = store.intern_node(SemanticNodeData::Primitive(
        crate::semantic_query::PrimitiveKind::Number,
    ));
    let target = store.intern_node(SemanticNodeData::Primitive(
        crate::semantic_query::PrimitiveKind::Number,
    ));

    // An empty carrier with no self-roots validates vacuously.
    let key = crate::semantic_query::RelateMemoKey::assignable(
        source,
        target,
        crate::semantic_query::RelationContext::default(),
    );
    store.insert_relation(
        key.clone(),
        ReadSetSignature::empty(),
        Arc::from(Vec::<Arc<str>>::new()),
        crate::semantic_query::RelationResult::Assignable {
            bindings: Arc::from(Vec::new().into_boxed_slice()),
        },
        host.project_type_store().current_project_generation(),
    );

    let cached = store.get_relation(ctx, &key);
    assert!(
        matches!(
            cached,
            Some((_, crate::semantic_query::RelationResult::Assignable { .. }))
        ),
        "the relation memo must serve a warm judgement whose carrier validates \
         (got {cached:?})",
    );
}

/// Family memo `MemoEntry::validate` MUST reject a stale-generation
/// entry after a bare `ProjectTypeStore::bump_project_generation()`.
///
/// Discrimination model:
///
/// 1. A `KeyOf` over a `Global`-scoped primitive admits a family memo
///    entry whose carrier carries NO file self-root (the structural
///    base has no `NodeScopeId::File`) — every cross-file dependency
///    fact rail is empty and the entry's `self_root_canonicals` is
///    empty. The cold-build's `dep_signature` fence is
///    `project_generation_signature()` — a single `<project>`
///    `DepVersion::ProjectGeneration { generation: g0 }`.
/// 2. The host bumps the project generation via the bare
///    `bump_project_generation()` (NOT `_and_evict()`). The bare bump
///    increments the counter; it does NOT clear the family memo, the
///    in-flight table, or any cache layer, so the published entry
///    physically survives.
/// 3. Without folding the dispatch's `ProjectGeneration` fence into the
///    published `ReadSetSignature.facts`, the entry's
///    `read_set_signature` carries no `FactVersionRef::ProjectGeneration`
///    and no `FactVersionRef::FileWholeHash` — `validate_with_self_roots`
///    iterates an empty fact rail and accepts vacuously. Stale-by-
///    generation entry warm-hits.
///
/// DISCRIMINATES: pre-fix, `graph.get_validated(&key, &host)` returns
/// `Some(...)` because the bare bump leaves the entry physically
/// resident and `validate_with_self_roots` accepts an empty fact rail.
/// Post-fix, `dep_signature_to_fact_signature(&output.dep_signature)`
/// folds the `<project>` `ProjectGeneration { generation: g0 }` into
/// the carrier's `facts`; `validate_with_self_roots` routes that
/// `FactVersionRef::ProjectGeneration` through `view.validates` which
/// compares to the live `project_generation` (now `g0 + 1`), returning
/// `false`. `get_validated` then returns `None` and the cold build is
/// allowed to recompute under the new generation.
#[test]
fn family_memo_validate_rejects_stale_project_generation() {
    use crate::semantic_query::{PrimitiveKind, QueryError, SemanticNodeData};

    let host = host();
    let ctx: &dyn ResolverContext = &host;
    let dispatch = host.semantic_dispatch();
    let graph = host.project_type_store().semantic_graph();

    let prim = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let key = SemanticQueryKey::KeyOf {
        base: prim,
        context: crate::semantic_query::ProjectionReductionContext::published(
            crate::semantic_query::ProjectionMode::Expanded,
        ),
    };
    let _ = dispatch.execute_type_node(key.clone());

    // Fixture invariants — the published entry has empty
    // self-root-canonicals and carries no FileWholeHash on its facts
    // rail. This is the precondition the discrimination relies on:
    // a strict `validate_with_self_roots` on an empty rail accepts
    // vacuously unless a `ProjectGeneration` fact has been folded
    // into the carrier.
    let carrier = graph
        .entry_read_set_signature_for_tests(&key)
        .expect("KeyOf over a Global-scoped primitive must admit a memo entry");
    assert!(
        !carrier
            .facts
            .iter()
            .any(|f| matches!(f, FactVersionRef::FileWholeHash { .. })),
        "fixture invariant: the published KeyOf entry's carrier holds NO \
         FileWholeHash — the base is Global-scoped, so no file self-root \
         contributes",
    );

    let g0 = host.project_type_store().current_project_generation();

    // Bare project-generation bump. NOT `_and_evict` — that path
    // would clear the memo and trivially make the warm read miss.
    // We need the entry to physically survive so the test discriminates
    // the VALIDATION gate, not a clear.
    let g1 = host.project_type_store().bump_project_generation();
    assert!(
        g1 > g0,
        "the bare bump_project_generation() must increment the counter",
    );

    // The entry physically survives the bare bump — `get_unvalidated`
    // still finds it.
    assert!(
        graph.get_unvalidated(&key).is_some(),
        "fixture invariant: bare bump_project_generation() must NOT clear \
         the family memo — the entry physically survives so the test \
         discriminates the VALIDATION gate, not a clear",
    );

    // The discriminating assertion. Pre-fix: carrier.facts is empty
    // (the dispatch's `ProjectGeneration` fence is NOT folded into
    // facts), so `validate_with_self_roots` accepts vacuously and
    // `get_validated` returns Some — the stale entry warm-hits.
    // Post-fix: `ProjectGeneration { generation: g0 }` rides in
    // carrier.facts, `view.validates` rejects it against the live
    // generation g1, and `get_validated` returns None.
    assert!(
        graph.get_validated(&key, ctx).is_none(),
        "FAMILY MEMO STALE-GENERATION WARM HIT: a bare \
         bump_project_generation() (no clear) MUST reject the stale \
         family memo entry. Without folding the dispatch's \
         `ProjectGeneration` fence into the carrier's facts rail, an \
         entry whose self-root canonicals and file-content facts are \
         empty validates vacuously even after a project-shape change \
         that the generation counter recorded. The fix folds \
         `dep_signature_to_fact_signature(&output.dep_signature)` into \
         the published `ReadSetSignature.facts` so \
         `FactVersionRef::ProjectGeneration` is present on the carrier \
         and `view.validates` rejects the entry naturally.",
    );

    // Ensure we do not coincidentally observe a `KeyOf` of a
    // primitive that builds to a non-Value (the build closure's `_ =>
    // self.opaque(QueryError::Miss)` arm interns a Miss node and the
    // wrapper still wraps that as `QueryResult::Value(node)`). If the
    // entry were unpublishable, `entry_read_set_signature_for_tests`
    // above would have returned `None` and the fixture would have
    // panicked. Sanity-check the surface: the build produced an
    // Opaque(Miss) interned id, not a recursive sentinel.
    if let Some(node_data) = graph.node_data(prim) {
        assert!(
            matches!(&*node_data, SemanticNodeData::Primitive(_)),
            "sanity: the base under test is the interned primitive id",
        );
    }
    let _ = QueryError::Miss; // keep the import live without an unused warning.
}

/// Helper-shape check: a structural `SemanticNodeId` (a `Global`-scoped
/// primitive) contributes no self-root — `observed_self_roots_from_nodes`
/// over structural inputs yields an empty set, so a node kind keyed on
/// only structural inputs publishes a carrier with no file self-root
/// and warm-validates vacuously.
#[test]
fn structural_node_kind_publishes_no_file_self_root() {
    let host = host();
    let ctx: &dyn ResolverContext = &host;
    let dispatch = host.semantic_dispatch();
    let graph = host.project_type_store().semantic_graph();

    // `keyof` of a structural primitive: the base is `Global`-scoped,
    // so the published `KeyOf` entry has no file self-root.
    let prim = graph.intern_node(SemanticNodeData::Primitive(
        crate::semantic_query::PrimitiveKind::String,
    ));
    let key = SemanticQueryKey::KeyOf {
        base: prim,
        context: crate::semantic_query::ProjectionReductionContext::published(
            crate::semantic_query::ProjectionMode::Expanded,
        ),
    };
    let _ = dispatch.execute_type_node(key.clone());

    if let Some(carrier) = graph.entry_read_set_signature_for_tests(&key) {
        assert!(
            !carrier
                .facts
                .iter()
                .any(|f| matches!(f, FactVersionRef::FileWholeHash { .. })),
            "a KeyOf over a structural (Global-scoped) primitive must publish a carrier \
             with NO file self-root FileWholeHash",
        );
        // A carrier with no self-roots validates vacuously.
        let entry = graph.get_validated(&key, ctx).map(|r| r.value);
        assert!(
            entry.is_some(),
            "a structural KeyOf entry (no file self-root) must warm-validate vacuously",
        );
    }
}

// ---------------------------------------------------------------------------
// Node kinds keyed by interned `SemanticNodeId`s — `ProjectPath` /
// `KeyOf` — were UNROOTED before self-version-rooting (their
// `dep_signature` was project-generation-only, so the fence carried no
// `FileWholeHash`). They now derive a self-root from the file-derived
// origin scope of each input node. These discriminators prove the
// self-root is recorded: a same-canonical edit to the input node's
// originating file rejects the warm entry.
// ---------------------------------------------------------------------------

/// Prime an `Instantiate` of a non-generic type so the result node is
/// an Object surface scoped to `canonical`'s file, and return that
/// file-derived node id.
fn file_derived_object_node(host: &VerterHost, canonical: &str) -> SemanticNodeId {
    let dispatch = host.semantic_dispatch();
    let _graph = host.project_type_store().semantic_graph();
    let _ = dispatch.execute_type_node(SemanticQueryKey::ResolveDecl(resolve_decl_key(
        canonical, "Foo",
    )));
    let _ = host
        .ensure_indexed_ready(canonical)
        .map(|indexed| indexed.whole_hash)
        .expect("file IndexedReady materialises");
    let key = SemanticQueryKey::Instantiate {
        base: crate::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
            Arc::from(canonical),
            Arc::from("Foo"),
        ),
        args: Arc::from(Vec::new().into_boxed_slice()),
        context: crate::semantic_query::InstantiateContext::new(
            crate::semantic_query::ProjectionReductionContext::published(
                crate::semantic_query::ProjectionMode::Expanded,
            ),
            Default::default(),
        ),
    };
    match dispatch.execute_type_node(key) {
        crate::semantic_query::QueryResult::Value(crate::semantic_query::SemanticQueryOutput {
            value: node,
            ..
        }) => node,
        other => panic!("Instantiate of a non-generic type must yield a Value, got {other:?}"),
    }
}

/// `KeyOf` — a same-canonical content edit to the base node's
/// originating file rejects the warm query-node memo entry.
///
/// Discriminating property: `build_key_of` was UNROOTED before
/// self-version-rooting — its `dep_signature` carried only
/// `ProjectGeneration`, so the entry's fence-derived carrier had NO
/// `FileWholeHash` and a same-canonical content edit (which does not
/// bump the project generation) left the warm `KeyOf` entry valid.
/// Self-version-rooting derives a self-root from the base node's
/// file-derived origin scope, so the edit now rejects the entry.
/// Reverting the self-root prepend in `semantic_graph_read_set_signature`
/// leaves the `KeyOf` carrier without a file self-root and this test
/// FAILS — the stale entry validates.
#[test]
fn key_of_same_canonical_edit_rejects_warm_entry() {
    let host = host();
    let c = "/sg_self_root/key_of.ts";
    upsert(&host, c, "export type Foo = { a: number; b: string };\n");
    let base = file_derived_object_node(&host, c);
    let dispatch = host.semantic_dispatch();
    let graph = host.project_type_store().semantic_graph();

    let key = SemanticQueryKey::KeyOf {
        base,
        context: crate::semantic_query::ProjectionReductionContext::published(
            crate::semantic_query::ProjectionMode::Expanded,
        ),
    };
    let primed = dispatch.execute_type_node(key.clone());
    assert!(
        matches!(primed, crate::semantic_query::QueryResult::Value(_)),
        "KeyOf over a file-derived Object base must resolve before the edit"
    );
    // Hard fixture invariant: `KeyOf` over the file-derived Object node
    // built by `file_derived_object_node` MUST warm a memo entry. An
    // early `return` here would make the test pass vacuously if the
    // `KeyOf` kind ever stopped publishing — the discrimination is the
    // validator rejecting the entry, not its absence.
    assert!(
        graph.get_unvalidated(&key).is_some(),
        "fixture invariant: the warm KeyOf memo entry must exist after priming"
    );

    // Edit the base node's originating file. The upsert performs no
    // own-canonical drain, so the `KeyOf` entry survives — the warm
    // read exercises the entry's self-root validation.
    upsert(
        &host,
        c,
        "export type Foo = { a: number; b: string; c: boolean };\n",
    );

    let ctx: &dyn ResolverContext = &host;
    assert!(
        graph.get_unvalidated(&key).is_some(),
        "the physical KeyOf memo entry must still be present after the same-canonical edit"
    );
    assert!(
        graph.get_validated(&key, ctx).is_none(),
        "KeyOf: a content edit to the base node's originating file MUST reject the warm \
         memo entry — self-version-rooting derives a self-root FileWholeHash from the base \
         node's origin scope; a `KeyOf` carrier without that self-root validates the stale \
         entry",
    );
}

/// `KeyOf` over a CROSS-FILE merged declaration — a content edit to an
/// AUGMENTER file (not the base file) rejects the warm `KeyOf` memo entry.
///
/// Discriminating property: the `build_key_of` `MergedDecl` arm must root
/// self-roots on the merged carrier PLUS every contributor. That carrier
/// is scoped to the base/importing file, but an AUGMENTED `MergedDecl`
/// (cross-file `declare module` / `declare global`) carries contributor
/// nodes lowered in AUGMENTER file scopes. Rooting on the carrier ALONE
/// records only the base file's `FileWholeHash`, so an augmenter content
/// edit (which leaves the base file untouched) leaves the warm `KeyOf`
/// entry VALID and warm-serves a stale keyspace. Rooting on the carrier
/// PLUS every contributor node (deduped) records the augmenter's
/// `FileWholeHash`, so the edit rejects the entry.
///
/// The merged carrier here is assembled from two REAL file-derived
/// `Object` contributors (one per upserted file) and interned in the base
/// file's scope — exactly the scope structure the production augmentation
/// stitch (`build.rs`) produces. Reverting the fix (rooting on `[base]`
/// only) leaves the augmenter self-root unrecorded and this test FAILS:
/// the augmenter edit no longer rejects the entry.
#[test]
fn key_of_over_cross_file_merged_decl_rejects_warm_entry_on_augmenter_edit() {
    let host = host();
    let base_c = "/sg_self_root/merged_base.ts";
    let aug_c = "/sg_self_root/merged_augmenter.ts";
    upsert(&host, base_c, "export type Foo = { a: number };\n");
    upsert(&host, aug_c, "export type Foo = { b: string };\n");

    let graph = host.project_type_store().semantic_graph();
    // Two REAL file-derived `Object` contributors, each scoped to its own
    // originating file (the augmenter contributor carries the augmenter
    // file's `FileWholeHash` in its origin scope).
    let base_node = file_derived_object_node(&host, base_c);
    let aug_node = file_derived_object_node(&host, aug_c);
    // Sanity: the contributors genuinely originate in DIFFERENT files, so a
    // base-only self-root truly omits the augmenter version.
    let base_scope = graph
        .node_scope(base_node)
        .expect("base contributor is file-scoped");
    let aug_scope = graph
        .node_scope(aug_node)
        .expect("augmenter contributor is file-scoped");
    assert_ne!(
        base_scope, aug_scope,
        "fixture invariant: the two contributors must originate in different files"
    );

    // Assemble the merged carrier in the BASE file's scope — the exact
    // shape the production stitch interns (`intern_node_with_scope(MergedDecl,
    // base_scope)`), with contributor nodes spanning two files.
    let contributors: Arc<[SemanticNodeId]> =
        Arc::from(vec![base_node, aug_node].into_boxed_slice());
    let merged =
        graph.intern_node_with_scope(SemanticNodeData::MergedDecl { contributors }, base_scope);

    let dispatch = host.semantic_dispatch();
    let key = SemanticQueryKey::KeyOf {
        base: merged,
        context: crate::semantic_query::ProjectionReductionContext::published(
            crate::semantic_query::ProjectionMode::Expanded,
        ),
    };
    let primed = dispatch.execute_type_node(key.clone());
    assert!(
        matches!(primed, crate::semantic_query::QueryResult::Value(_)),
        "KeyOf over a cross-file merged declaration must resolve before the edit"
    );
    let ctx: &dyn ResolverContext = &host;
    // Fixture invariant: the warm `KeyOf` entry exists AND validates BEFORE
    // any edit — the discrimination is the validator rejecting it after the
    // augmenter edit, not its absence.
    assert!(
        graph.get_unvalidated(&key).is_some(),
        "fixture invariant: the warm KeyOf memo entry must exist after priming"
    );
    assert!(
        graph.get_validated(&key, ctx).is_some(),
        "fixture invariant: the warm KeyOf entry must validate before any edit"
    );

    // Edit ONLY the augmenter file. The base file is untouched, so a
    // carrier-only (base) self-root would still validate. The
    // upsert performs no own-canonical drain on the KeyOf slot, so the
    // entry physically survives and the warm read exercises self-root
    // validation directly.
    upsert(
        &host,
        aug_c,
        "export type Foo = { b: string; c: boolean };\n",
    );

    assert!(
        graph.get_unvalidated(&key).is_some(),
        "the physical KeyOf memo entry must still be present after the augmenter edit \
         — the discrimination is the validator rejecting it, not its absence"
    );
    assert!(
        graph.get_validated(&key, ctx).is_none(),
        "KeyOf over a cross-file merged declaration MUST reject the warm entry on an \
         AUGMENTER edit — the entry must root on the carrier PLUS every contributor node, \
         so the augmenter contributor's FileWholeHash is recorded; rooting on the base \
         carrier alone warm-serves a stale keyspace",
    );
}

/// `ProjectPath` — a same-canonical content edit to the base node's
/// originating file rejects the warm query-node memo entry.
///
/// Discriminating property: identical to the `KeyOf` case —
/// `build_project_path` was UNROOTED before self-version-rooting (its
/// `dep_signature` carried only `ProjectGeneration`). Self-version-
/// rooting derives a self-root from the projection `base`'s file-
/// derived origin scope. Reverting the self-root prepend leaves the
/// `ProjectPath` carrier without a file self-root and this test FAILS.
#[test]
fn project_path_same_canonical_edit_rejects_warm_entry() {
    let host = host();
    let c = "/sg_self_root/project_path.ts";
    upsert(&host, c, "export type Foo = { a: number; b: string };\n");
    let base = file_derived_object_node(&host, c);
    let dispatch = host.semantic_dispatch();
    let graph = host.project_type_store().semantic_graph();

    let key = SemanticQueryKey::ProjectPath {
        base,
        path: Arc::from(
            vec![crate::semantic_query::PathSegment::Member(Arc::from("a"))].into_boxed_slice(),
        ),
        context: crate::semantic_query::ProjectionReductionContext::published(
            crate::semantic_query::ProjectionMode::Navigate,
        ),
    };
    let primed = dispatch.execute_type_node(key.clone());
    assert!(
        matches!(primed, crate::semantic_query::QueryResult::Value(_)),
        "ProjectPath `.a` over a file-derived Object base must resolve before the edit"
    );
    // Hard fixture invariant: projecting member `a` of the file-derived
    // Object node built by `file_derived_object_node` MUST warm a memo
    // entry. An early `return` here would make the test pass vacuously
    // if the `ProjectPath` kind ever stopped publishing — the
    // discrimination is the validator rejecting the entry, not its
    // absence.
    assert!(
        graph.get_unvalidated(&key).is_some(),
        "fixture invariant: the warm ProjectPath memo entry must exist after priming"
    );

    upsert(
        &host,
        c,
        "export type Foo = { a: number; b: string; c: boolean };\n",
    );

    let ctx: &dyn ResolverContext = &host;
    assert!(
        graph.get_unvalidated(&key).is_some(),
        "the physical ProjectPath memo entry must still be present after the same-canonical edit"
    );
    assert!(
        graph.get_validated(&key, ctx).is_none(),
        "ProjectPath: a content edit to the projection base's originating file MUST reject \
         the warm memo entry via the base node's file-derived self-root",
    );
}

// ---------------------------------------------------------------------------
// Cold-owner carrier bubble — a parent that COLD-builds a nested child
// must end with the SAME dep coverage as a parent that WARM-HIT the
// child.
// ---------------------------------------------------------------------------

/// The cold-owner publish path bubbles the completed carrier into the
/// still-active outer tracer — exactly as the warm-hit fast path and the
/// in-flight joiner path already do.
///
/// Discriminating property: a `ResolveDecl` cold build observes NO facts
/// organically onto the tracer (`build_resolve_decl` reads the scope
/// `IndexedReady` through `ensure_indexed_ready` — a storage accessor
/// that fans nothing — then builds the dep-signature and interns a
/// placeholder). The build's self-root `FileWholeHash` is **synthesised**
/// by `semantic_graph_read_set_signature` onto the published carrier; it
/// is never an `observe_fan_out` observation. So an outer tracer wrapping
/// a *cold* dispatch sees the child's self-root `FileWholeHash` ONLY if
/// the cold-owner publish path bubbles the carrier.
///
/// Pre-fix the cold owner path published the carrier to the memo entry
/// and to the in-flight joiner state, but never bubbled it into this
/// thread's still-active parent tracer — so a parent that cold-built a
/// child accumulated strictly fewer deps than a parent that warm-hit the
/// same child (the warm-hit and joiner paths both bubble). Reverting the
/// `carrier.bubble(ctx)` in `execute_cooperative_slow`'s cold-owner path
/// leaves the outer tracer's finalised signature WITHOUT the cold child's
/// self-root `FileWholeHash` and this test FAILS.
///
/// The test also asserts the cold-built-child coverage EQUALS the
/// warm-hit-child coverage: a second dispatch of the same key under a
/// fresh outer tracer warm-hits and bubbles the identical self-root fact.
#[test]
fn cold_owner_bubbles_carrier_into_outer_tracer() {
    let host = host();
    let c = "/sg_self_root/cold_owner_bubble.ts";
    upsert(&host, c, "export type Foo = { a: number };\n");
    let observed_hash = host
        .ensure_indexed_ready(c)
        .map(|indexed| indexed.whole_hash)
        .expect("declaring file IndexedReady materialises");
    let key = SemanticQueryKey::ResolveDecl(resolve_decl_key(c, "Foo"));

    // Outer tracer #1 wraps the FIRST dispatch of `key` — a cold build.
    // The cold-owner publish path must bubble the freshly-built carrier
    // (whose facts rail leads with the self-root `FileWholeHash` for `c`)
    // into this outer tracer.
    let ((), cold_finalise, _) = crate::fact_signature_helpers::install_fact_tracer(&host, || {
        let dispatch = host.semantic_dispatch();
        let r = dispatch.execute_type_node(key.clone());
        assert!(
            matches!(r, crate::semantic_query::QueryResult::Value(_)),
            "the cold ResolveDecl dispatch must resolve to a Value"
        );
    });

    let cold_facts = match cold_finalise {
        FactReadSetFinalise::Ok(sig) => sig,
        FactReadSetFinalise::Overflow => {
            panic!("outer tracer overflowed — a single ResolveDecl cold build cannot overflow")
        }
    };
    assert!(
        cold_facts.iter().any(|f| matches!(
            f,
            FactVersionRef::FileWholeHash { canonical_id, hash }
                if canonical_id == c && *hash == observed_hash
        )),
        "the cold-owner publish path MUST bubble the completed carrier into the still-active \
         outer tracer — the outer tracer's finalised signature must contain the cold child's \
         synthesised self-root FileWholeHash for {c}. Pre-fix the cold owner published the \
         carrier to the memo + joiner state but never bubbled it into the parent tracer, so a \
         cold-built child under-rooted the parent. Got: {cold_facts:?}",
    );

    // Outer tracer #2 wraps the SECOND dispatch of the SAME key — now a
    // warm hit. The warm-hit fast path bubbles the entry's carrier. The
    // warm-hit-child coverage MUST equal the cold-built-child coverage:
    // a parent's dep set is path-independent regardless of whether the
    // child was cold or warm.
    let ((), warm_finalise, _) = crate::fact_signature_helpers::install_fact_tracer(&host, || {
        let dispatch = host.semantic_dispatch();
        let _ = dispatch.execute_type_node(key.clone());
    });
    let warm_facts = match warm_finalise {
        FactReadSetFinalise::Ok(sig) => sig,
        FactReadSetFinalise::Overflow => panic!("warm outer tracer overflowed — setup error"),
    };
    assert!(
        warm_facts.iter().any(|f| matches!(
            f,
            FactVersionRef::FileWholeHash { canonical_id, hash }
                if canonical_id == c && *hash == observed_hash
        )),
        "the warm-hit fast path bubbles the entry's carrier — the outer tracer must contain \
         the same self-root FileWholeHash for {c}. Got: {warm_facts:?}",
    );
}

// ---------------------------------------------------------------------------
// Built-in utility instantiations rooted on their argument files.
// ---------------------------------------------------------------------------

/// A built-in utility instantiation (`Pick<Foo, 'a'>`) self-roots on the
/// file its source argument `Foo` was lowered from — a content edit to
/// that file rejects the warm utility memo entry.
///
/// Discriminating property: the built-in utility branch in
/// `build_instantiate` previously returned the `QueryBuildOutput` with
/// **empty** `observed_self_roots`, even though `build_builtin_utility`
/// inspects the argument nodes to form the result (`Pick` reads the
/// source's Object surface and enumerates the key set). With empty
/// `observed_self_roots` the published carrier carried no self-root
/// `FileWholeHash` for the source argument's file, so a same-canonical
/// content edit to that file left the warm utility entry valid and a
/// reissue served the stale result.
///
/// The fix derives the self-roots from the `args` node set via
/// `observed_self_roots_from_nodes`. Reverting that derivation leaves the
/// `Pick` instantiation entry without the source-file self-root and this
/// test FAILS — the stale entry validates after the edit.
#[test]
fn builtin_utility_instantiation_roots_on_argument_file() {
    let host = host();
    let c = "/sg_self_root/builtin_utility_arg.ts";
    upsert(&host, c, "export type Foo = { a: number; b: string };\n");
    // `file_derived_object_node` primes an `Instantiate` of the
    // non-generic `Foo` so the result node is an Object surface scoped to
    // `c`'s file — a genuine file-derived argument for the utility.
    let source = file_derived_object_node(&host, c);
    let dispatch = host.semantic_dispatch();
    let graph = host.project_type_store().semantic_graph();

    // `Pick<Foo, 'a'>` — args are [source_object_node, key_set]. The
    // key set is a structural literal union (Global-scoped → contributes
    // no self-root); the source object node is file-derived.
    let members = vec![Arc::<str>::from("a")];
    let key_set = dispatch.intern_string_literal_union(&members);
    let key = SemanticQueryKey::Instantiate {
        base: crate::project_semantic_dispatch::pick_builtin_decl_identity(),
        args: Arc::from(vec![source, key_set].into_boxed_slice()),
        context: crate::semantic_query::InstantiateContext::new(
            crate::semantic_query::ProjectionReductionContext::published(
                crate::semantic_query::ProjectionMode::Expanded,
            ),
            Default::default(),
        ),
    };

    let primed = dispatch.execute_type_node(key.clone());
    assert!(
        matches!(primed, crate::semantic_query::QueryResult::Value(_)),
        "the Pick<Foo, 'a'> built-in utility instantiation must resolve before the edit"
    );
    // Hard fixture invariant: the `Pick` utility instantiation MUST warm
    // a memo entry. An early `return` here would make the test pass
    // vacuously if the kind ever stopped publishing — the discrimination
    // is the validator rejecting the entry, not its absence.
    assert!(
        graph.get_unvalidated(&key).is_some(),
        "fixture invariant: the warm Pick utility memo entry must exist after priming"
    );
    // Direct carrier inspection: the published entry's facts rail MUST
    // lead with the source argument's file self-root `FileWholeHash`.
    // This is the precise discrimination — pre-fix the carrier had no
    // FileWholeHash at all.
    let carrier = graph
        .entry_read_set_signature_for_tests(&key)
        .expect("the Pick utility memo entry must carry a ReadSetSignature");
    assert!(
        carrier.facts.iter().any(|f| matches!(
            f,
            FactVersionRef::FileWholeHash { canonical_id, .. } if canonical_id == c
        )),
        "the built-in utility instantiation carrier MUST carry a self-root FileWholeHash for \
         the source argument's file {c} — `build_builtin_utility` inspects the argument nodes \
         to form the result, so the result depends on each file-derived argument's file. \
         Pre-fix the utility branch returned empty `observed_self_roots` and the carrier had \
         no FileWholeHash. Got facts: {:?}",
        carrier.facts,
    );

    // Edit the source argument's file. The upsert performs no
    // own-canonical drain, so the utility entry survives — the warm
    // read exercises the entry's self-root validation directly.
    upsert(
        &host,
        c,
        "export type Foo = { a: number; b: string; d: boolean };\n",
    );

    let ctx: &dyn ResolverContext = &host;
    assert!(
        graph.get_unvalidated(&key).is_some(),
        "the physical Pick utility memo entry must still be present after the same-canonical edit"
    );
    assert!(
        graph.get_validated(&key, ctx).is_none(),
        "built-in utility instantiation: a content edit to the source argument's originating \
         file MUST reject the warm utility memo entry — the entry self-roots on the \
         argument's file via `observed_self_roots_from_nodes` over the `args` node set",
    );
}

/// A NON-builtin generic instantiation `Gen<Arg>` self-roots on the file
/// its file-derived type argument `Arg` was lowered from — a content edit
/// to that file (which is NOT the declaring file of `Gen`) rejects the
/// warm `Instantiate` memo entry.
///
/// Discriminating property: the non-builtin `Instantiate` path in
/// `build_instantiate` rooted its `observed_self_roots` ONLY on the
/// declaring file (`DeclIdentity.canonical_id` + `whole_hash`). The
/// instantiated shell substitutes the generic `args` into the
/// declaration body, so the result transitively depends on the file
/// each file-derived argument was lowered from — but a content edit to
/// an argument's file does NOT shift the *declaring* file's whole hash,
/// so the declaring-file-only carrier validated the stale entry. The
/// built-in utility path was fixed (see
/// `builtin_utility_instantiation_roots_on_argument_file`); the
/// non-builtin path was missed.
///
/// The fix merges `observed_self_roots_from_nodes(args)` alongside the
/// declaration-file root. Reverting that merge leaves the non-builtin
/// `Instantiate` carrier without the argument-file self-root and this
/// test FAILS — the stale entry validates after the edit. The two files
/// are distinct (`gen` declares `Box<T>`; `arg` declares `Foo`, used as
/// the type argument) so the discrimination is precisely the
/// argument-file self-root, not the declaring-file one.
#[test]
fn non_builtin_instantiation_roots_on_type_argument_file() {
    let host = host();
    let gen = "/sg_self_root/non_builtin_inst_gen.ts";
    let arg = "/sg_self_root/non_builtin_inst_arg.ts";
    // Declaring file: a userland generic `Box<T>` (NOT a built-in
    // utility — `utility_source` classifies `Box` as userland, so
    // `build_instantiate` takes the `resolve_prepared_type_decl` path).
    upsert(&host, gen, "export type Box<T> = { value: T };\n");
    // Argument file: a separate canonical whose `Foo` is lowered into a
    // genuine file-derived Object node scoped to `arg`'s file.
    upsert(&host, arg, "export type Foo = { a: number; b: string };\n");
    let arg_node = file_derived_object_node(&host, arg);
    let dispatch = host.semantic_dispatch();
    let graph = host.project_type_store().semantic_graph();

    let _ = host
        .ensure_indexed_ready(gen)
        .map(|indexed| indexed.whole_hash)
        .expect("declaring file IndexedReady materialises");
    // `Box<Foo>` — the single arg is the file-derived `Foo` Object node
    // scoped to `arg`'s file. The declaring file is `gen`.
    let key = SemanticQueryKey::Instantiate {
        base: crate::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
            Arc::from(gen),
            Arc::from("Box"),
        ),
        args: Arc::from(vec![arg_node].into_boxed_slice()),
        context: crate::semantic_query::InstantiateContext::new(
            crate::semantic_query::ProjectionReductionContext::published(
                crate::semantic_query::ProjectionMode::Expanded,
            ),
            Default::default(),
        ),
    };

    let primed = dispatch.execute_type_node(key.clone());
    assert!(
        matches!(primed, crate::semantic_query::QueryResult::Value(_)),
        "the Box<Foo> non-builtin instantiation must resolve before the edit"
    );
    // Hard fixture invariant: the non-builtin `Instantiate` MUST warm a
    // memo entry. An early `return` would make the test pass vacuously
    // if the kind ever stopped publishing — the discrimination is the
    // validator rejecting the entry, not its absence.
    assert!(
        graph.get_unvalidated(&key).is_some(),
        "fixture invariant: the warm non-builtin Instantiate memo entry must exist after priming"
    );
    // Direct carrier inspection: the published entry's facts rail MUST
    // lead with the type argument's file self-root `FileWholeHash` for
    // `arg` — this is the precise discrimination. Pre-fix the carrier
    // carried a `FileWholeHash` only for the declaring file `gen`.
    let carrier = graph
        .entry_read_set_signature_for_tests(&key)
        .expect("the non-builtin Instantiate memo entry must carry a ReadSetSignature");
    assert!(
        carrier.facts.iter().any(|f| matches!(
            f,
            FactVersionRef::FileWholeHash { canonical_id, .. } if canonical_id == arg
        )),
        "the non-builtin instantiation carrier MUST carry a self-root FileWholeHash for the \
         type argument's file {arg} — `build_instantiate` substitutes the generic `args` into \
         the declaration body, so the result depends on each file-derived argument's file. \
         Pre-fix the non-builtin path rooted only on the declaring file. Got facts: {:?}",
        carrier.facts,
    );

    // Edit the type argument's file. The upsert performs no
    // own-canonical drain, so the `Instantiate` entry survives — the
    // warm read exercises the entry's self-root validation directly.
    // The declaring file `gen` is UNTOUCHED, so a declaring-file-only
    // carrier would still validate; only the argument-file self-root
    // catches this edit.
    upsert(
        &host,
        arg,
        "export type Foo = { a: number; b: string; c: boolean };\n",
    );

    let ctx: &dyn ResolverContext = &host;
    assert!(
        graph.get_unvalidated(&key).is_some(),
        "the physical non-builtin Instantiate memo entry must still be present after the edit"
    );
    assert!(
        graph.get_validated(&key, ctx).is_none(),
        "non-builtin instantiation: a content edit to the type argument's originating file \
         (NOT the declaring file) MUST reject the warm Instantiate memo entry — the entry \
         self-roots on each file-derived argument's file via `observed_self_roots_from_nodes` \
         over the `args` node set, merged alongside the declaring-file root",
    );
}

/// A `ResolveMacroPayload` whose macro type argument is file-derived
/// from another canonical self-roots on that argument's file — a content
/// edit to the argument's file (which is NOT the owning SFC) rejects the
/// warm `ResolveMacroPayload` memo entry.
///
/// Discriminating property: `build_resolve_macro_payload` rooted its
/// `observed_self_roots` ONLY on the macro owner (the owner file's
/// canonical + its re-sourced whole_hash). The `DefineProps` 1-arg arm returns the
/// `type_args[0]` node directly, so the macro payload's identity IS the
/// type argument — when that argument is file-derived from another
/// canonical the result transitively depends on that file's content.
/// But a content edit to the argument's file does NOT shift the owning
/// SFC's whole hash, so the owner-only carrier validated the stale
/// entry.
///
/// The fix merges `observed_self_roots_from_nodes(type_args)` alongside
/// the owner root. Reverting that merge leaves the `ResolveMacroPayload`
/// carrier without the argument-file self-root and this test FAILS — the
/// stale entry validates after the edit. The owning SFC and the
/// argument file are distinct canonicals so the discrimination is
/// precisely the type-argument self-root, not the owner one.
#[test]
fn resolve_macro_payload_roots_on_type_argument_file() {
    let host = host();
    let sfc = "/sg_self_root/macro_payload_owner.vue";
    let arg = "/sg_self_root/macro_payload_arg.ts";
    // Owning SFC — a `defineProps` macro. The `ResolveMacroPayload` key
    // is owned by this canonical.
    upsert(
        &host,
        sfc,
        "<script setup lang=\"ts\">defineProps<{ x: string }>()</script>\n",
    );
    // Type-argument file — a separate canonical whose `Foo` lowers into
    // a genuine file-derived Object node scoped to `arg`'s file.
    upsert(&host, arg, "export type Foo = { a: number; b: string };\n");
    let arg_node = file_derived_object_node(&host, arg);
    let dispatch = host.semantic_dispatch();
    let graph = host.project_type_store().semantic_graph();

    let _ = host
        .ensure_indexed_ready(sfc)
        .map(|indexed| indexed.whole_hash)
        .expect("owner SFC IndexedReady materialises");
    // A `DefineProps` macro with a SINGLE type argument: the 1-arg arm
    // returns `type_args[0]` directly, so the resolved payload IS the
    // file-derived `Foo` Object node scoped to `arg`'s file. The owning
    // canonical is `sfc`.
    let key = SemanticQueryKey::ResolveMacroPayload {
        owner: crate::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
            Arc::from(sfc),
            Arc::from("<sfc-script-setup>"),
        ),
        macro_index: 0,
        macro_kind: verter_semantic::analysis::AnalyzedMacroKind::DefineProps,
        type_args: Arc::from(vec![arg_node].into_boxed_slice()),
        context: crate::semantic_query::MacroPayloadContext::new(
            Default::default(),
            crate::semantic_query::ProjectionMode::Expanded,
        ),
    };

    let primed = dispatch.execute_type_node(key.clone());
    assert!(
        matches!(primed, crate::semantic_query::QueryResult::Value(_)),
        "the ResolveMacroPayload over a file-derived type argument must resolve before the edit"
    );
    // Hard fixture invariant: the `ResolveMacroPayload` MUST warm a memo
    // entry. An early `return` would make the test pass vacuously if the
    // kind ever stopped publishing — the discrimination is the validator
    // rejecting the entry, not its absence.
    assert!(
        graph.get_unvalidated(&key).is_some(),
        "fixture invariant: the warm ResolveMacroPayload memo entry must exist after priming"
    );
    // Direct carrier inspection: the published entry's facts rail MUST
    // lead with the type argument's file self-root `FileWholeHash` for
    // `arg` — the precise discrimination. Pre-fix the carrier carried a
    // `FileWholeHash` only for the owning SFC `sfc`.
    let carrier = graph
        .entry_read_set_signature_for_tests(&key)
        .expect("the ResolveMacroPayload memo entry must carry a ReadSetSignature");
    assert!(
        carrier.facts.iter().any(|f| matches!(
            f,
            FactVersionRef::FileWholeHash { canonical_id, .. } if canonical_id == arg
        )),
        "the ResolveMacroPayload carrier MUST carry a self-root FileWholeHash for the type \
         argument's file {arg} — the `DefineProps` 1-arg arm returns the `type_args[0]` node \
         directly, so the payload's identity depends on that file's content. Pre-fix the \
         carrier rooted only on the owning SFC. Got facts: {:?}",
        carrier.facts,
    );

    // Edit the type argument's file. The upsert performs no
    // own-canonical drain, so the `ResolveMacroPayload` entry survives
    // — the warm read exercises the entry's self-root validation
    // directly. The owning SFC is UNTOUCHED, so an owner-only carrier
    // would still validate; only the argument-file self-root catches it.
    upsert(
        &host,
        arg,
        "export type Foo = { a: number; b: string; c: boolean };\n",
    );

    let ctx: &dyn ResolverContext = &host;
    assert!(
        graph.get_unvalidated(&key).is_some(),
        "the physical ResolveMacroPayload memo entry must still be present after the edit"
    );
    assert!(
        graph.get_validated(&key, ctx).is_none(),
        "ResolveMacroPayload: a content edit to the macro type argument's originating file \
         (NOT the owning SFC) MUST reject the warm memo entry — the entry self-roots on each \
         file-derived `type_args` node's file via `observed_self_roots_from_nodes`, merged \
         alongside the owner root",
    );
}

// ---------------------------------------------------------------------------
// Session-overlay warm-hit validation matrix.
// ---------------------------------------------------------------------------

/// `execute_cooperative`'s warm-hit fast path validates a session
/// warm hit against the **overlay** content identity, not the base
/// host's.
///
/// A cold build under a [`SessionResolverContext`] roots the
/// semantic-graph `MemoEntry`'s self-root `FileWholeHash` on the
/// OVERLAY content hash — `build_resolve_decl` / `build_typeof`
/// observe `ensure_indexed_ready`, which under a session resolves the
/// overlay `IndexedReady` (`overlay_hash`). The warm-hit validator
/// (`MemoEntry::validate` → `validate_with_self_roots` →
/// `validate_fact_signature_with_self_roots` →
/// `view.validates_self_root_whole_hash`) routes the self-root through
/// `ctx.resolver_store_view_read().into_owned_view()`. Pre-fix `SessionResolverContext::
/// resolver_store_view` delegated straight to the base host, whose
/// `HostStoreView.whole_hashes` records the BASE content hash — so an
/// overlay-rooted self-root never matched and every session-overlay
/// warm read cold-recomputed. Post-fix `resolver_store_view` applies
/// `HostStoreView::with_session_overlay`, which overrides
/// `whole_hashes[canonical]` with the overlay content hash for every
/// overlay-bearing canonical.
///
/// This single test discriminates the full hit/miss matrix the fix
/// must satisfy. Each case publishes one `MemoEntry` whose carrier and
/// `self_root_canonicals` mirror exactly what the cold build produces
/// (a leading self-root `FileWholeHash` for the keyed canonical), then
/// drives `execute_cooperative` and observes the `cold_ran` closure
/// flag:
///
/// - **overlay-current** — entry rooted on the *current* overlay hash,
///   queried under the session context → **warm HIT** (`cold_ran`
///   stays false). This assertion is RED pre-fix: the base store view
///   rejects the overlay-rooted self-root, so the cold build re-runs.
/// - **overlay-stale** — entry rooted on a *superseded* overlay hash
///   (the overlay was edited since), queried under the session context
///   → **MISS** (`cold_ran` becomes true). Proves the overlay-aware
///   view is not a blanket accept — it validates against the session's
///   *current* overlay identity.
/// - **base-rooted-under-overlay** — entry rooted on the *base* content
///   hash, queried under a session that NOW has an overlay for that
///   canonical → **MISS**. The base-content value is stale relative to
///   the overlay.
/// - **base/base** — entry rooted on the base hash, queried under the
///   plain base host context (no session) → **warm HIT** (unchanged).
#[test]
fn session_overlay_warm_validation_matrix() {
    use crate::resolver_core::SessionResolverContext;
    use crate::semantic_query::QueryResult;
    use crate::session_view::{OverlaidView, SessionView};
    use rustc_hash::FxHashMap;

    let canonical = "/sg_overlay/probe.ts";
    // Base file: materialised on the host under the base content hash.
    let host = host();
    upsert(
        &host,
        canonical,
        "export interface Probe { base: number; }\nexport const probe = 1;\n",
    );
    let base_hash = host
        .ensure_indexed_ready(canonical)
        .expect("base IndexedReady must materialise for the upserted file")
        .whole_hash;
    let host = Arc::new(host);

    // Overlay source: deliberately different bytes → different content
    // hash, so the base and overlay artifacts are distinguishable.
    let overlay_source: Arc<str> =
        Arc::from("export interface Probe { overlay: string; }\nexport const probe = 2;\n");
    let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
    overlays.insert(canonical.to_string(), Arc::clone(&overlay_source));
    let view = OverlaidView::new(Arc::clone(&host), overlays);
    let overlay_hash = view
        .overlay_content_hash_for(canonical)
        .expect("OverlaidView must report an overlay content hash for the masked canonical");
    assert_ne!(
        overlay_hash, base_hash,
        "fixture invariant: the overlay source differs from the base, so its content hash \
         must differ — otherwise the matrix cases are indistinguishable",
    );

    // Publish the overlay `IndexedReady` candidate under the
    // overlay-scoped key so the session view tracks an overlay
    // artifact for the canonical (multi-candidate sibling of the base
    // artifact).
    let overlay_indexed = host
        .materialize_overlay_indexed_ready_with_view(canonical, &view)
        .expect("overlay IndexedReady must materialise");
    assert_eq!(
        overlay_indexed.whole_hash, overlay_hash,
        "fixture invariant: the overlay artifact is keyed by the overlay hash",
    );

    // A genuinely-different *superseded* overlay hash — the content
    // hash of an older overlay source. Distinct from both the current
    // overlay hash and the base hash, so the overlay-stale case is a
    // real stale-version mismatch, not a synthetic sentinel.
    let stale_overlay_hash = crate::hash::hash_16(
        b"export interface Probe { overlay: boolean; }\nexport const probe = 0;\n",
    );
    assert_ne!(stale_overlay_hash, overlay_hash);
    assert_ne!(stale_overlay_hash, base_hash);

    // The `ResolveDecl` key for the keyed canonical — the family-memo
    // key shape `build_resolve_decl` cold-publishes.
    let key = SemanticQueryKey::ResolveDecl(resolve_decl_key(canonical, "Probe"));

    // Build a `MemoEntry` carrier self-rooted on `root_hash`, exactly
    // as `semantic_graph_read_set_signature` does for a cold build:
    // the fact rail leads with the keyed canonical's self-root
    // `FileWholeHash`, and `self_root_canonicals` lists that canonical.
    let publish_entry_rooted_on = |graph: &SemanticGraphStore, root_hash: [u8; 16]| {
        let node = graph.intern_node(SemanticNodeData::Primitive(
            crate::semantic_query::PrimitiveKind::String,
        ));
        let carrier = ReadSetSignature::new(Arc::from(vec![FactVersionRef::FileWholeHash {
            canonical_id: canonical.to_string(),
            hash: root_hash,
        }]));
        let self_roots: Arc<[Arc<str>]> = Arc::from(vec![Arc::<str>::from(canonical)]);
        let published = graph.publish_with_carrier_for_tests(
            key.clone(),
            QueryResult::Value(node),
            carrier,
            self_roots,
        );
        assert!(
            published > 0,
            "fixture invariant: the warm memo entry must be published",
        );
        node
    };

    // `execute_cooperative` driver: returns `(cold_ran, node)`. The
    // cold build interns and returns a *fresh* recompute node so the
    // caller can tell a warm hit (returns the published node) from a
    // cold recompute (returns the recompute node) independently of the
    // `cold_ran` flag.
    let drive = |graph: &SemanticGraphStore, ctx: &dyn ResolverContext| {
        let mut cold_ran = false;
        let recompute_node = graph.intern_node(SemanticNodeData::Primitive(
            crate::semantic_query::PrimitiveKind::Number,
        ));
        let read = graph.execute_cooperative(
            ctx,
            key.clone(),
            || {
                graph.intern_node(SemanticNodeData::Opaque(
                    crate::semantic_query::QueryError::Miss,
                ))
            },
            || {
                cold_ran = true;
                (
                    QueryResult::Value(recompute_node),
                    Arc::from(Vec::new().into_boxed_slice()) as DepSignature,
                )
            },
        );
        (cold_ran, read.value, recompute_node)
    };

    // --- Case 1: overlay-current → warm HIT ------------------------
    {
        let graph = SemanticGraphStore::new();
        let published = publish_entry_rooted_on(&graph, overlay_hash);
        let session_store_view = host
            .resolver_store_view_read()
            .into_owned_view()
            .with_session_overlay(&host, &view);
        let session_ctx = SessionResolverContext::new(
            &host,
            &view,
            &session_store_view,
            std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
        );
        let (cold_ran, value, _recompute) = drive(&graph, &session_ctx);
        assert!(
            !cold_ran,
            "overlay-current: an entry self-rooted on the CURRENT overlay hash, queried \
             under a SessionResolverContext that overlays the canonical, MUST warm-HIT. \
             Pre-fix `SessionResolverContext::resolver_store_view` delegates to the base \
             host, whose store view records the BASE hash for the canonical — the strict \
             self-root validator then rejects the overlay-rooted entry and the cold build \
             re-runs. This is the exact defect: every session-overlay warm read \
             cold-recomputes.",
        );
        match value {
            QueryResult::Value(node) => assert_eq!(
                node, published,
                "overlay-current: the warm hit must surface the PUBLISHED node",
            ),
            other => panic!("overlay-current: expected the published Value, got {other:?}"),
        }
    }

    // --- Case 2: overlay-stale → MISS ------------------------------
    {
        let graph = SemanticGraphStore::new();
        let _published = publish_entry_rooted_on(&graph, stale_overlay_hash);
        let session_store_view = host
            .resolver_store_view_read()
            .into_owned_view()
            .with_session_overlay(&host, &view);
        let session_ctx = SessionResolverContext::new(
            &host,
            &view,
            &session_store_view,
            std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
        );
        let (cold_ran, value, recompute) = drive(&graph, &session_ctx);
        assert!(
            cold_ran,
            "overlay-stale: an entry self-rooted on a SUPERSEDED overlay hash (the overlay \
             was edited since) MUST MISS and re-run cold. The overlay-aware store view \
             validates the self-root against the session's CURRENT overlay content hash — \
             it is NOT a blanket 'accept anything under a session'. A warm hit here would \
             reintroduce stale-serving.",
        );
        match value {
            QueryResult::Value(node) => assert_eq!(
                node, recompute,
                "overlay-stale: the recomputed node must surface, not the stale entry",
            ),
            other => panic!("overlay-stale: expected the recomputed Value, got {other:?}"),
        }
    }

    // --- Case 3: base-rooted-under-overlay → MISS ------------------
    {
        let graph = SemanticGraphStore::new();
        let _published = publish_entry_rooted_on(&graph, base_hash);
        let session_store_view = host
            .resolver_store_view_read()
            .into_owned_view()
            .with_session_overlay(&host, &view);
        let session_ctx = SessionResolverContext::new(
            &host,
            &view,
            &session_store_view,
            std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
        );
        let (cold_ran, value, recompute) = drive(&graph, &session_ctx);
        assert!(
            cold_ran,
            "base-rooted-under-overlay: an entry self-rooted on the BASE content hash, \
             queried under a session that NOW has an overlay for that canonical, MUST \
             MISS — the base-content value is stale relative to the overlay. The \
             overlay-aware view records the overlay hash for the canonical, so the \
             base-rooted self-root no longer validates.",
        );
        match value {
            QueryResult::Value(node) => assert_eq!(
                node, recompute,
                "base-rooted-under-overlay: the recomputed node must surface",
            ),
            other => {
                panic!("base-rooted-under-overlay: expected the recomputed Value, got {other:?}")
            }
        }
    }

    // --- Case 4: base/base → warm HIT (unchanged) ------------------
    {
        let graph = SemanticGraphStore::new();
        let published = publish_entry_rooted_on(&graph, base_hash);
        // Plain base host context — no session overlay.
        let base_ctx: &dyn ResolverContext = host.as_ref();
        let (cold_ran, value, _recompute) = drive(&graph, base_ctx);
        assert!(
            !cold_ran,
            "base/base: an entry self-rooted on the base hash, queried under the plain \
             base host context, MUST warm-HIT — the base store view records the base \
             hash for the canonical. This case is unchanged by the fix; it guards \
             against the overlay-aware path leaking into non-session contexts.",
        );
        match value {
            QueryResult::Value(node) => assert_eq!(
                node, published,
                "base/base: the warm hit must surface the PUBLISHED node",
            ),
            other => panic!("base/base: expected the published Value, got {other:?}"),
        }
    }
}

/// `execute_cooperative`'s warm-hit fast path validates a session
/// warm hit whose carrier carries a **`Parse` fact** against the
/// **overlay** content identity, not the base host's.
///
/// `session_overlay_warm_validation_matrix` above covers a carrier
/// whose only fact is the self-root `FileWholeHash`. That fact routes
/// through `validates_self_root_whole_hash`, which closes the
/// self-root validation by re-rooting `HostStoreView.whole_hashes`.
/// A `Parse` fact routes through a DIFFERENT validator —
/// `validates_parse_domain`, which reads the per-canonical
/// `Arc<FileFacts>` from `HostStoreView.file_facts`.
/// `HostStoreView::build` snapshots `file_facts` from the **base**
/// content; `with_session_overlay` re-roots only
/// `whole_hashes`. So a warm `MemoEntry` whose carrier holds a `Parse`
/// fact for an overlaid canonical — the fact's `expected_hash` is the
/// fact hash live in the **overlay** artifact, because a cold build
/// under a session pins parse facts to the overlay content version —
/// validated the overlay `expected_hash` against the **base**
/// `FileFacts` snapshot and missed on every call.
///
/// This test discriminates the `Parse`-fact hit/miss matrix:
///
/// - **overlay-current (`Parse` fact)** — the carrier's self-root
///   `FileWholeHash` AND its `Parse(SyntacticExportSet)` fact are both
///   pinned to the current overlay version → **warm HIT**. RED
///   pre-fix-4: the `Parse` fact validates against the base
///   `FileFacts` snapshot (whose `SyntacticExportSet` hash differs),
///   so the cold build re-runs even though `whole_hashes` is
///   overlay-rooted.
/// - **overlay-stale (`Parse` fact)** — the carrier's self-root is the
///   current overlay hash but the `Parse` fact carries a *superseded*
///   hash (the base version's `SyntacticExportSet` hash) → **MISS**.
///   Proves the refreshed `file_facts` is not a blanket accept — it
///   rejects a `Parse` fact pinned to a non-current content version.
/// - **base/base (`Parse` fact)** — the carrier's self-root AND
///   `Parse` fact are pinned to the base version, queried under the
///   plain base host context → **warm HIT** (unchanged). Guards the
///   `file_facts` refresh against leaking into non-session contexts.
#[test]
fn session_overlay_parse_fact_carrier_warm_validation() {
    use crate::resolver_core::SessionResolverContext;
    use crate::semantic_query::QueryResult;
    use crate::session_view::{OverlaidView, SessionView};
    use rustc_hash::FxHashMap;

    let canonical = "/sg_overlay_parse/probe.ts";
    let host = host();
    // Base file. Export name set: { Probe, probe }.
    upsert(
        &host,
        canonical,
        "export interface Probe { base: number; }\nexport const probe = 1;\n",
    );
    let base_hash = host
        .ensure_indexed_ready(canonical)
        .expect("base IndexedReady must materialise")
        .whole_hash;
    let host = Arc::new(host);

    // Overlay source: adds a third export (`extra`) so the
    // `SyntacticExportSet` parse fact — the export NAME set — genuinely
    // differs from the base. A members-only edit would leave the
    // export-name set identical and the matrix cases indistinguishable.
    let overlay_source: Arc<str> = Arc::from(
        "export interface Probe { overlay: string; }\nexport const probe = 2;\nexport const extra = 3;\n",
    );
    let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
    overlays.insert(canonical.to_string(), Arc::clone(&overlay_source));
    let view = OverlaidView::new(Arc::clone(&host), overlays);
    let overlay_hash = view
        .overlay_content_hash_for(canonical)
        .expect("OverlaidView must report an overlay content hash");
    assert_ne!(
        overlay_hash, base_hash,
        "fixture invariant: overlay differs"
    );

    // Publish the overlay `IndexedReady` + `FileArtifacts` candidate
    // under the overlay-scoped key (multi-candidate sibling of the
    // base).
    let _overlay_indexed = host
        .materialize_overlay_indexed_ready_with_view(canonical, &view)
        .expect("overlay IndexedReady must materialise");

    // Recover the `SyntacticExportSet` parse-fact hash for each content
    // version directly from the content-addressed artifact store —
    // exactly as `parse_fact_ref_for_observed_current_content` does for
    // a cold build (provenance-pure, content-addressed at the observed
    // hash). `base_ctx` is the plain host (any `ResolverContext` works —
    // the helper is content-addressed, not view-dependent).
    let base_ctx: &dyn ResolverContext = host.as_ref();
    let base_parse_fact =
        crate::fact_signature_helpers::parse_fact_ref_for_observed_current_content(
            base_ctx,
            canonical,
            base_hash,
            FactKey::SyntacticExportSet,
            FactLane::Semantic,
        )
        .expect("base SyntacticExportSet parse fact must resolve");
    let overlay_parse_fact =
        crate::fact_signature_helpers::parse_fact_ref_for_observed_current_content(
            base_ctx,
            canonical,
            overlay_hash,
            FactKey::SyntacticExportSet,
            FactLane::Semantic,
        )
        .expect("overlay SyntacticExportSet parse fact must resolve");
    assert_ne!(
        base_parse_fact.expected_hash, overlay_parse_fact.expected_hash,
        "fixture invariant: the overlay adds an export, so its \
         SyntacticExportSet parse-fact hash must differ from the base's \
         — otherwise the overlay-current vs overlay-stale cases are \
         indistinguishable and the test does not discriminate",
    );

    let key = SemanticQueryKey::ResolveDecl(resolve_decl_key(canonical, "Probe"));

    // Publish a `MemoEntry` whose carrier's fact rail leads with the
    // self-root `FileWholeHash` pinned to `root_hash` and ALSO carries a
    // `Parse(SyntacticExportSet)` fact pinned to `parse_fact`. This is
    // the carrier shape a cold build produces when its tracer observed a
    // syntactic-export-set parse fact for the keyed canonical.
    let publish_entry = |graph: &SemanticGraphStore,
                         root_hash: [u8; 16],
                         parse_fact: &crate::resolver_core::ParseFactRef| {
        let node = graph.intern_node(SemanticNodeData::Primitive(
            crate::semantic_query::PrimitiveKind::String,
        ));
        let carrier = ReadSetSignature::new(Arc::from(vec![
            FactVersionRef::FileWholeHash {
                canonical_id: canonical.to_string(),
                hash: root_hash,
            },
            FactVersionRef::Parse(parse_fact.clone()),
        ]));
        let self_roots: Arc<[Arc<str>]> = Arc::from(vec![Arc::<str>::from(canonical)]);
        let published = graph.publish_with_carrier_for_tests(
            key.clone(),
            QueryResult::Value(node),
            carrier,
            self_roots,
        );
        assert!(published > 0, "fixture invariant: the warm entry publishes");
        node
    };

    let drive = |graph: &SemanticGraphStore, ctx: &dyn ResolverContext| {
        let mut cold_ran = false;
        let recompute_node = graph.intern_node(SemanticNodeData::Primitive(
            crate::semantic_query::PrimitiveKind::Number,
        ));
        let read = graph.execute_cooperative(
            ctx,
            key.clone(),
            || {
                graph.intern_node(SemanticNodeData::Opaque(
                    crate::semantic_query::QueryError::Miss,
                ))
            },
            || {
                cold_ran = true;
                (
                    QueryResult::Value(recompute_node),
                    Arc::from(Vec::new().into_boxed_slice()) as DepSignature,
                )
            },
        );
        (cold_ran, read.value, recompute_node)
    };

    // --- Case 1: overlay-current (Parse fact) → warm HIT -----------
    {
        let graph = SemanticGraphStore::new();
        let published = publish_entry(&graph, overlay_hash, &overlay_parse_fact);
        let session_store_view = host
            .resolver_store_view_read()
            .into_owned_view()
            .with_session_overlay(&host, &view);
        let session_ctx = SessionResolverContext::new(
            &host,
            &view,
            &session_store_view,
            std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
        );
        let (cold_ran, value, _recompute) = drive(&graph, &session_ctx);
        assert!(
            !cold_ran,
            "overlay-current (Parse fact): an entry whose carrier holds \
             the self-root FileWholeHash AND a Parse(SyntacticExportSet) \
             fact, both pinned to the CURRENT overlay version, MUST warm-HIT \
             under a SessionResolverContext. Pre-fix-4 `with_session_overlay` \
             re-rooted only `whole_hashes`; `file_facts` stayed snapshotted \
             from the base content, so `validates_parse_domain` compared the \
             overlay parse-fact hash against the base FileFacts and missed — \
             every session-overlay warm read with a Parse-fact carrier \
             cold-recomputed.",
        );
        match value {
            QueryResult::Value(node) => assert_eq!(
                node, published,
                "overlay-current (Parse fact): the warm hit surfaces the PUBLISHED node",
            ),
            other => {
                panic!("overlay-current (Parse fact): expected the published Value, got {other:?}")
            }
        }
    }

    // --- Case 2: overlay-stale (Parse fact) → MISS -----------------
    {
        let graph = SemanticGraphStore::new();
        // Self-root on the current overlay hash, but the Parse fact
        // carries the BASE version's SyntacticExportSet hash — a
        // genuine superseded-version mismatch.
        let stale_parse_fact = crate::resolver_core::ParseFactRef {
            canonical_id: canonical.to_string(),
            key: FactKey::SyntacticExportSet,
            lane: FactLane::Semantic,
            expected_hash: base_parse_fact.expected_hash,
        };
        let _published = publish_entry(&graph, overlay_hash, &stale_parse_fact);
        let session_store_view = host
            .resolver_store_view_read()
            .into_owned_view()
            .with_session_overlay(&host, &view);
        let session_ctx = SessionResolverContext::new(
            &host,
            &view,
            &session_store_view,
            std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
        );
        let (cold_ran, value, recompute) = drive(&graph, &session_ctx);
        assert!(
            cold_ran,
            "overlay-stale (Parse fact): an entry whose self-root is the \
             current overlay hash but whose Parse fact carries a SUPERSEDED \
             SyntacticExportSet hash MUST MISS. The overlay-refreshed \
             `file_facts` validates the Parse fact against the overlay's \
             CURRENT FileFacts — it is NOT a blanket 'accept any Parse fact \
             under a session overlay'.",
        );
        match value {
            QueryResult::Value(node) => assert_eq!(
                node, recompute,
                "overlay-stale (Parse fact): the recomputed node must surface",
            ),
            other => {
                panic!("overlay-stale (Parse fact): expected the recomputed Value, got {other:?}")
            }
        }
    }

    // --- Case 3: base/base (Parse fact) → warm HIT (unchanged) -----
    {
        let graph = SemanticGraphStore::new();
        let published = publish_entry(&graph, base_hash, &base_parse_fact);
        let base_ctx: &dyn ResolverContext = host.as_ref();
        let (cold_ran, value, _recompute) = drive(&graph, base_ctx);
        assert!(
            !cold_ran,
            "base/base (Parse fact): an entry whose self-root AND Parse fact \
             are pinned to the base version, queried under the plain base \
             host context, MUST warm-HIT — the base store view's `file_facts` \
             holds the base FileFacts. Guards the overlay `file_facts` refresh \
             against leaking into non-session contexts.",
        );
        match value {
            QueryResult::Value(node) => assert_eq!(
                node, published,
                "base/base (Parse fact): the warm hit surfaces the PUBLISHED node",
            ),
            other => panic!("base/base (Parse fact): expected the published Value, got {other:?}"),
        }
    }
}

/// `execute_cooperative`'s warm-hit fast path rejects a warm entry
/// rooted on a canonical the session has DELETED (overlay-Deleted).
///
/// `HostStoreView::with_session_overlay` re-roots /
/// removes per-canonical snapshots by iterating `overlay_canonicals()`
/// — overlay-*source* keys only. A canonical the session tombstoned
/// (deleted) but never re-upserted is absent from that set, so the
/// tombstone branch never ran for it: the base `whole_hashes` /
/// `file_facts` / `derived_hashes` snapshots survived in the
/// validation view, and a warm `MemoEntry` rooted on the (now-deleted)
/// base file still validated. The fix iterates
/// [`SessionView::tombstoned_canonicals`] in addition to
/// `overlay_canonicals()` and drops every per-canonical snapshot for a
/// tombstoned canonical, so a strict validation rejects the entry.
///
/// The tombstone-bearing view is `OverlaidViewRef` — the sole
/// `SessionView` impl that carries a tombstone set. The view here has
/// an EMPTY overlay-source map and a tombstone for the probe canonical
/// (the file was deleted, not re-upserted) — exactly the shape
/// `MetaSession::with_overlay_view` produces for a `SessionOverlay::Delete`.
///
/// Discrimination per case:
///
/// - **deleted (`FileWholeHash` self-root)** — an entry self-rooted on
///   the deleted file's base content hash, queried under the session
///   that deleted it → **MISS** (`cold_ran` true). RED pre-fix:
///   `with_session_overlay` skips the tombstone-only canonical, the
///   base hash stays in `whole_hashes`, and
///   `validates_self_root_whole_hash` accepts the base-rooted entry —
///   a warm hit serving a deleted file. GREEN post-fix: `whole_hashes`
///   no longer has the canonical, the strict self-root validator
///   rejects, the cold build re-runs.
/// - **deleted (`Parse` carrier)** — an entry whose carrier holds a
///   real `Parse(SyntacticExportSet)` fact for the deleted canonical →
///   **MISS**. RED pre-fix: `file_facts` keeps the base `FileFacts`
///   snapshot, so `validates_parse_domain` matches the base parse-fact
///   hash and the stale entry serves. GREEN post-fix: `file_facts` no
///   longer has the canonical; `validates_parse_domain` rejects a real
///   (non-zero) fact hash for an untracked file.
/// - **sibling-not-deleted** — a DIFFERENT canonical, untouched by the
///   session, with an entry self-rooted on ITS current content,
///   queried under the same tombstone-bearing view → **warm HIT**.
///   Proves the fix drops snapshots only for the tombstoned canonical
///   — it is not a blanket reject of every entry under a session that
///   deleted some file.
#[test]
fn session_tombstone_rejects_base_rooted_warm_entry() {
    use crate::resolver_core::SessionResolverContext;
    use crate::semantic_query::QueryResult;
    use crate::session_view::{OverlaidViewRef, SessionView};
    use rustc_hash::FxHashMap;

    let deleted_canonical = "/sg_tombstone/probe.ts";
    let sibling_canonical = "/sg_tombstone/sibling.ts";
    let host = host();
    // The probe file is alive on the base host — a warm entry was
    // produced against this content before the session deleted it.
    upsert(
        &host,
        deleted_canonical,
        "export interface Probe { base: number; }\nexport const probe = 1;\n",
    );
    // A sibling file the session does NOT touch — its warm entries
    // must still validate under the tombstone-bearing session.
    upsert(
        &host,
        sibling_canonical,
        "export interface Sibling { kept: string; }\nexport const sibling = 2;\n",
    );
    let deleted_base_hash = host
        .ensure_indexed_ready(deleted_canonical)
        .expect("deleted-file base IndexedReady must materialise")
        .whole_hash;
    let sibling_base_hash = host
        .ensure_indexed_ready(sibling_canonical)
        .expect("sibling-file base IndexedReady must materialise")
        .whole_hash;
    let host = Arc::new(host);

    // `OverlaidViewRef` with an EMPTY overlay-source map and a single
    // tombstone for the probe canonical — the session deleted the
    // file and did not re-upsert it.
    let overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
    let overlay_hashes: FxHashMap<String, crate::types::Hash16> = FxHashMap::default();
    let mut overlay_tombstones: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    overlay_tombstones.insert(deleted_canonical.to_string());
    let view = OverlaidViewRef::new(&host, &overlays, &overlay_hashes, &overlay_tombstones);
    assert!(
        view.is_tombstoned(deleted_canonical),
        "fixture invariant: the view tombstones the probe canonical",
    );
    assert!(
        !view.is_tombstoned(sibling_canonical),
        "fixture invariant: the sibling canonical is NOT tombstoned",
    );
    assert!(
        view.overlay_canonicals().is_empty(),
        "fixture invariant: the view carries NO overlay-source keys — the \
         file was deleted, not re-upserted. This is exactly why iterating \
         only `overlay_canonicals()` misses the tombstone.",
    );
    assert_eq!(
        view.tombstoned_canonicals(),
        vec![deleted_canonical.to_string()],
        "fixture invariant: the tombstone set reports the deleted canonical",
    );

    // The base parse fact for the deleted canonical's SyntacticExportSet
    // — the carrier shape a cold build produced while the file was alive.
    let base_ctx: &dyn ResolverContext = host.as_ref();
    let deleted_parse_fact =
        crate::fact_signature_helpers::parse_fact_ref_for_observed_current_content(
            base_ctx,
            deleted_canonical,
            deleted_base_hash,
            FactKey::SyntacticExportSet,
            FactLane::Semantic,
        )
        .expect("base SyntacticExportSet parse fact for the deleted file must resolve");
    assert_ne!(
        deleted_parse_fact.expected_hash, [0u8; 16],
        "fixture invariant: the deleted file has a real (non-zero) \
         SyntacticExportSet hash — a zero sentinel would make the \
         parse-domain validator's untracked-accept window fire and the \
         Parse-fact case would not discriminate",
    );

    // Publish a `MemoEntry` for `key` whose carrier is `facts`,
    // self-rooted on `self_root`.
    let publish = |graph: &SemanticGraphStore,
                   key: &SemanticQueryKey,
                   facts: Vec<FactVersionRef>,
                   self_root: &str| {
        let node = graph.intern_node(SemanticNodeData::Primitive(
            crate::semantic_query::PrimitiveKind::String,
        ));
        let carrier = ReadSetSignature::new(Arc::from(facts));
        let self_roots: Arc<[Arc<str>]> = Arc::from(vec![Arc::<str>::from(self_root)]);
        let published = graph.publish_with_carrier_for_tests(
            key.clone(),
            QueryResult::Value(node),
            carrier,
            self_roots,
        );
        assert!(published > 0, "fixture invariant: the warm entry publishes");
        node
    };

    let drive = |graph: &SemanticGraphStore, key: &SemanticQueryKey, ctx: &dyn ResolverContext| {
        let mut cold_ran = false;
        let recompute_node = graph.intern_node(SemanticNodeData::Primitive(
            crate::semantic_query::PrimitiveKind::Number,
        ));
        let read = graph.execute_cooperative(
            ctx,
            key.clone(),
            || {
                graph.intern_node(SemanticNodeData::Opaque(
                    crate::semantic_query::QueryError::Miss,
                ))
            },
            || {
                cold_ran = true;
                (
                    QueryResult::Value(recompute_node),
                    Arc::from(Vec::new().into_boxed_slice()) as DepSignature,
                )
            },
        );
        (cold_ran, read.value, recompute_node)
    };

    // --- Case 1: deleted (FileWholeHash self-root) → MISS ----------
    {
        let graph = SemanticGraphStore::new();
        let key = SemanticQueryKey::ResolveDecl(resolve_decl_key(deleted_canonical, "Probe"));
        let _published = publish(
            &graph,
            &key,
            vec![FactVersionRef::FileWholeHash {
                canonical_id: deleted_canonical.to_string(),
                hash: deleted_base_hash,
            }],
            deleted_canonical,
        );
        let session_store_view = host
            .resolver_store_view_read()
            .into_owned_view()
            .with_session_overlay(&host, &view);
        let session_ctx = SessionResolverContext::new(
            &host,
            &view,
            &session_store_view,
            std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
        );
        let (cold_ran, value, recompute) = drive(&graph, &key, &session_ctx);
        assert!(
            cold_ran,
            "deleted (FileWholeHash self-root): an entry self-rooted on the \
             deleted file's base content hash, queried under the session that \
             deleted the file, MUST MISS and re-run cold — the file is gone. \
             RED pre-fix: `with_session_overlay` iterates only \
             `overlay_canonicals()`, which is EMPTY for a delete-only session, \
             so the tombstone branch never runs; the base hash survives in \
             `whole_hashes` and `validates_self_root_whole_hash` accepts the \
             base-rooted entry — a warm hit serving a file the session deleted.",
        );
        match value {
            QueryResult::Value(node) => assert_eq!(
                node, recompute,
                "deleted (FileWholeHash self-root): the recomputed node must surface",
            ),
            other => panic!(
                "deleted (FileWholeHash self-root): expected the recomputed Value, got {other:?}"
            ),
        }
    }

    // --- Case 2: deleted (Parse carrier) → MISS --------------------
    {
        let graph = SemanticGraphStore::new();
        let key = SemanticQueryKey::ResolveDecl(resolve_decl_key(deleted_canonical, "Probe"));
        let _published = publish(
            &graph,
            &key,
            vec![
                FactVersionRef::FileWholeHash {
                    canonical_id: deleted_canonical.to_string(),
                    hash: deleted_base_hash,
                },
                FactVersionRef::Parse(deleted_parse_fact.clone()),
            ],
            deleted_canonical,
        );
        let session_store_view = host
            .resolver_store_view_read()
            .into_owned_view()
            .with_session_overlay(&host, &view);
        let session_ctx = SessionResolverContext::new(
            &host,
            &view,
            &session_store_view,
            std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
        );
        let (cold_ran, value, recompute) = drive(&graph, &key, &session_ctx);
        assert!(
            cold_ran,
            "deleted (Parse carrier): an entry whose carrier holds a real \
             Parse(SyntacticExportSet) fact for the deleted canonical MUST \
             MISS. RED pre-fix: `with_session_overlay` skips the tombstone-only \
             canonical, so `file_facts` keeps the deleted file's base \
             `FileFacts` snapshot and `validates_parse_domain` matches the \
             base parse-fact hash — the stale entry serves a deleted file.",
        );
        match value {
            QueryResult::Value(node) => assert_eq!(
                node, recompute,
                "deleted (Parse carrier): the recomputed node must surface",
            ),
            other => {
                panic!("deleted (Parse carrier): expected the recomputed Value, got {other:?}")
            }
        }
    }

    // --- Case 3: sibling-not-deleted → warm HIT --------------------
    {
        let graph = SemanticGraphStore::new();
        let key = SemanticQueryKey::ResolveDecl(resolve_decl_key(sibling_canonical, "Sibling"));
        let published = publish(
            &graph,
            &key,
            vec![FactVersionRef::FileWholeHash {
                canonical_id: sibling_canonical.to_string(),
                hash: sibling_base_hash,
            }],
            sibling_canonical,
        );
        let session_store_view = host
            .resolver_store_view_read()
            .into_owned_view()
            .with_session_overlay(&host, &view);
        let session_ctx = SessionResolverContext::new(
            &host,
            &view,
            &session_store_view,
            std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
        );
        let (cold_ran, value, _recompute) = drive(&graph, &key, &session_ctx);
        assert!(
            !cold_ran,
            "sibling-not-deleted: a DIFFERENT canonical the session never \
             touched, with an entry self-rooted on ITS current content, \
             queried under the same tombstone-bearing view, MUST warm-HIT. \
             The fix drops per-canonical snapshots ONLY for the tombstoned \
             canonical — it is not a blanket reject of every entry under a \
             session that deleted some other file.",
        );
        match value {
            QueryResult::Value(node) => assert_eq!(
                node, published,
                "sibling-not-deleted: the warm hit surfaces the PUBLISHED node",
            ),
            other => panic!("sibling-not-deleted: expected the published Value, got {other:?}"),
        }
    }
}

/// A warm `MemoEntry` whose carrier carries a **cross-file**
/// (non-self-root) `FileWholeHash` dependency on a child canonical
/// MISSES when the session has TOMBSTONED (deleted) that child.
///
/// `HostStoreView::with_session_overlay` removes a
/// tombstoned canonical from `whole_hashes`. The strict self-root
/// validator (`validates_self_root_whole_hash`) then rejects an entry
/// *self-rooted* on the deleted file — correct. But a plain cross-file
/// `FileWholeHash` dependency fact routes through
/// [`crate::resolver_core::StoreView::validates`], whose `FileWholeHash`
/// arm keeps the lazy `None => true` "untracked → optimistically
/// accept" rule (cross-file permissiveness). A tombstoned canonical,
/// removed from `whole_hashes`, looks UNTRACKED to that arm — so a
/// parent memo entry carrying a cross-file `FileWholeHash` on a
/// session-deleted child still validated and served stale.
///
/// The fix records every tombstoned canonical in a dedicated
/// `HostStoreView` tombstone set; `validates(FileWholeHash)` rejects a
/// tombstoned canonical before the lazy untracked-accept rule.
///
/// Discrimination per case:
///
/// - **child-deleted** — a parent entry self-rooted on the LIVE parent
///   canonical (NOT tombstoned, hash unchanged) whose carrier also
///   holds a cross-file `FileWholeHash` for the child, queried under a
///   session that tombstones ONLY the child → **MISS** (`cold_ran`
///   true). RED pre-fix: the child's `FileWholeHash` hits the lazy
///   `None => true` untracked-accept arm and the parent self-root
///   validates strictly, so the stale parent entry serves a result
///   depending on a deleted file. GREEN post-fix: the tombstone set
///   makes `validates(FileWholeHash)` reject the tombstoned child, so
///   the parent warm read misses and the cold build re-runs.
/// - **child-kept** — the SAME parent entry shape, but the session
///   tombstones a DIFFERENT (unrelated) canonical, leaving the child
///   alive → **warm HIT**. Proves the rejection is scoped to the
///   tombstoned canonical: a parent whose cross-file dependency is
///   genuinely live still validates under a delete-bearing session.
#[test]
fn session_tombstone_rejects_cross_file_dependency_whole_hash() {
    use crate::resolver_core::SessionResolverContext;
    use crate::semantic_query::QueryResult;
    use crate::session_view::OverlaidViewRef;
    use rustc_hash::FxHashMap;

    let parent_canonical = "/sg_tombstone_dep/parent.ts";
    let child_canonical = "/sg_tombstone_dep/child.ts";
    let unrelated_canonical = "/sg_tombstone_dep/unrelated.ts";
    let host = host();
    // The parent and child files are alive on the base host — a warm
    // entry was produced against this content before the session
    // deleted the child.
    upsert(
        &host,
        parent_canonical,
        "export interface Parent { p: number; }\nexport const parent = 1;\n",
    );
    upsert(
        &host,
        child_canonical,
        "export interface Child { c: string; }\nexport const child = 2;\n",
    );
    upsert(&host, unrelated_canonical, "export const unrelated = 3;\n");
    let parent_base_hash = host
        .ensure_indexed_ready(parent_canonical)
        .expect("parent-file base IndexedReady must materialise")
        .whole_hash;
    let child_base_hash = host
        .ensure_indexed_ready(child_canonical)
        .expect("child-file base IndexedReady must materialise")
        .whole_hash;
    let host = Arc::new(host);

    // Publish a `MemoEntry` for `key` whose carrier holds the parent's
    // self-root `FileWholeHash` PLUS a cross-file `FileWholeHash`
    // dependency on `child` — `self_root_canonicals` lists ONLY the
    // parent, so the child fact routes through the lazy `validates`
    // path, not the strict self-root path.
    let publish = |graph: &SemanticGraphStore, key: &SemanticQueryKey| {
        let node = graph.intern_node(SemanticNodeData::Primitive(
            crate::semantic_query::PrimitiveKind::String,
        ));
        let carrier = ReadSetSignature::new(Arc::from(vec![
            FactVersionRef::FileWholeHash {
                canonical_id: parent_canonical.to_string(),
                hash: parent_base_hash,
            },
            FactVersionRef::FileWholeHash {
                canonical_id: child_canonical.to_string(),
                hash: child_base_hash,
            },
        ]));
        // Self-root is the PARENT only — the child is a cross-file dep.
        let self_roots: Arc<[Arc<str>]> = Arc::from(vec![Arc::<str>::from(parent_canonical)]);
        let published = graph.publish_with_carrier_for_tests(
            key.clone(),
            QueryResult::Value(node),
            carrier,
            self_roots,
        );
        assert!(published > 0, "fixture invariant: the warm entry publishes");
        node
    };

    let drive = |graph: &SemanticGraphStore, key: &SemanticQueryKey, ctx: &dyn ResolverContext| {
        let mut cold_ran = false;
        let recompute_node = graph.intern_node(SemanticNodeData::Primitive(
            crate::semantic_query::PrimitiveKind::Number,
        ));
        let read = graph.execute_cooperative(
            ctx,
            key.clone(),
            || {
                graph.intern_node(SemanticNodeData::Opaque(
                    crate::semantic_query::QueryError::Miss,
                ))
            },
            || {
                cold_ran = true;
                (
                    QueryResult::Value(recompute_node),
                    Arc::from(Vec::new().into_boxed_slice()) as DepSignature,
                )
            },
        );
        (cold_ran, read.value, recompute_node)
    };

    let overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
    let overlay_hashes: FxHashMap<String, crate::types::Hash16> = FxHashMap::default();

    // --- Case 1: child-deleted → parent warm read MISSES -----------
    {
        let graph = SemanticGraphStore::new();
        let key = SemanticQueryKey::ResolveDecl(resolve_decl_key(parent_canonical, "Parent"));
        let _published = publish(&graph, &key);
        let mut tombstones: std::collections::HashSet<String> = std::collections::HashSet::new();
        tombstones.insert(child_canonical.to_string());
        let view = OverlaidViewRef::new(&host, &overlays, &overlay_hashes, &tombstones);
        assert!(
            view.is_tombstoned(child_canonical),
            "fixture invariant: the view tombstones the child canonical",
        );
        assert!(
            !view.is_tombstoned(parent_canonical),
            "fixture invariant: the parent canonical is NOT tombstoned",
        );
        let session_store_view = host
            .resolver_store_view_read()
            .into_owned_view()
            .with_session_overlay(&host, &view);
        let session_ctx = SessionResolverContext::new(
            &host,
            &view,
            &session_store_view,
            std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
        );
        let (cold_ran, value, recompute) = drive(&graph, &key, &session_ctx);
        assert!(
            cold_ran,
            "child-deleted: a parent entry self-rooted on the LIVE parent \
             canonical whose carrier ALSO holds a cross-file `FileWholeHash` \
             dependency on the child MUST MISS when the session deleted the \
             child. RED pre-fix: the child fact routes through `validates`'s \
             `FileWholeHash` arm; `with_session_overlay` removed the \
             tombstoned child from `whole_hashes`, so the arm's lazy \
             `None => true` untracked-accept fires and the stale parent \
             entry serves a result depending on a deleted file. GREEN \
             post-fix: the `HostStoreView` tombstone set makes \
             `validates(FileWholeHash)` reject the tombstoned child.",
        );
        match value {
            QueryResult::Value(node) => assert_eq!(
                node, recompute,
                "child-deleted: the recomputed node must surface, not the stale entry",
            ),
            other => panic!("child-deleted: expected the recomputed Value, got {other:?}"),
        }
    }

    // --- Case 2: child-kept → parent warm read HITS ----------------
    {
        let graph = SemanticGraphStore::new();
        let key = SemanticQueryKey::ResolveDecl(resolve_decl_key(parent_canonical, "Parent"));
        let published = publish(&graph, &key);
        // The session tombstones an UNRELATED canonical — the child
        // dependency stays alive.
        let mut tombstones: std::collections::HashSet<String> = std::collections::HashSet::new();
        tombstones.insert(unrelated_canonical.to_string());
        let view = OverlaidViewRef::new(&host, &overlays, &overlay_hashes, &tombstones);
        assert!(
            !view.is_tombstoned(child_canonical),
            "fixture invariant: the child canonical is NOT tombstoned in case 2",
        );
        let session_store_view = host
            .resolver_store_view_read()
            .into_owned_view()
            .with_session_overlay(&host, &view);
        let session_ctx = SessionResolverContext::new(
            &host,
            &view,
            &session_store_view,
            std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
        );
        let (cold_ran, value, _recompute) = drive(&graph, &key, &session_ctx);
        assert!(
            !cold_ran,
            "child-kept: the SAME parent entry, under a session that \
             tombstones a DIFFERENT (unrelated) canonical, MUST warm-HIT — \
             the child dependency is genuinely live. The tombstone \
             rejection is scoped to the tombstoned canonical, not a \
             blanket reject of every cross-file `FileWholeHash` under a \
             delete-bearing session.",
        );
        match value {
            QueryResult::Value(node) => assert_eq!(
                node, published,
                "child-kept: the warm hit surfaces the PUBLISHED node",
            ),
            other => panic!("child-kept: expected the published Value, got {other:?}"),
        }
    }
}
