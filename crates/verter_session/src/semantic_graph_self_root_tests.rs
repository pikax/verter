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
//! 2. Edits the entry's defining canonical through
//!    [`crate::VerterHost::upsert_skipping_own_canonical_drain_for_tests`]
//!    — the skip-own-drain hook runs the full upsert pipeline but
//!    suppresses the own-canonical query-identity cache drain, so the
//!    warm graph entry is NOT removed by the drain. The test therefore
//!    observes whether the entry detects the same-canonical edit on
//!    its own self-root.
//! 3. Asserts the warm read (`get_validated`) MISSES — the strict
//!    self-root validator rejects the entry because the file's whole
//!    hash shifted — while `get_unvalidated` still finds the (now
//!    stale) physical entry.
//!
//! Discriminating property: pre-self-version-rooting, the
//! `SemanticGraphStore` query nodes did NOT validate a self-root
//! strictly — a warm read after a same-canonical edit (with the
//! own-canonical drain skipped) would return the stale entry. With
//! strict self-root validation the warm read misses. Each test's
//! `get_unvalidated`-still-`Some` assertion proves the entry was
//! physically present (the discrimination is the validator rejecting
//! it, not the entry being absent).

use std::sync::Arc;

use verter_semantic::facts::{FactKey, FactLane};

use crate::fact_signature_helpers::ReadSetSignature;
use crate::resolver_core::{FactVersionRef, ResolverContext};
use crate::semantic_query::{
    DepSignature, DepVersion, ResolveDeclKey, ScopeId, SemanticNodeData, SemanticNodeId,
    SemanticQueryApi, SemanticQueryKey,
};
use crate::semantic_query_memo::{semantic_graph_read_set_signature, SemanticGraphStore};
use crate::{FileKind, HostConfig, UpsertRequest, VerterHost};

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

/// Upsert through the production path (own-canonical drain runs).
fn upsert(host: &VerterHost, path: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(path.to_string()),
            input_id: path.to_string(),
            source: Arc::from(source),
            file_kind: FileKind::from_path(path),
            aliases: Vec::new(),
        })
        .expect("upsert succeeds");
}

/// Upsert through the test-only skip-own-canonical-drain hook so the
/// upserted canonical's own query-identity cache entries are NOT
/// drained — the warm graph entry survives the upsert and the test
/// can observe whether its self-root validation detects the edit.
fn upsert_skip_drain(host: &VerterHost, path: &str, source: &str) {
    let _ = host
        .upsert_skipping_own_canonical_drain_for_tests(UpsertRequest {
            canonical_id: Some(path.to_string()),
            input_id: path.to_string(),
            source: Arc::from(source),
            file_kind: FileKind::from_path(path),
            aliases: Vec::new(),
        })
        .expect("skip-drain upsert succeeds");
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
/// `scope.canonical_id`'s `FileWholeHash`. After a skip-own-drain edit
/// the file's whole hash shifts; the strict self-root validator
/// rejects the warm entry. Pre-self-version-rooting the entry was not
/// strictly self-rooted and the warm read (with the own-canonical
/// drain skipped) returned it stale.
#[test]
fn resolve_decl_same_canonical_edit_rejects_warm_entry() {
    let host = host();
    let c = "/sg_self_root/resolve_decl.ts";
    upsert(&host, c, "export type Foo = { a: number };\n");
    let dispatch = host.semantic_dispatch();
    let key = SemanticQueryKey::ResolveDecl(resolve_decl_key(c, "Foo"));

    // Prime the warm memo entry.
    let primed = dispatch.execute(key.clone());
    assert!(
        matches!(primed, crate::semantic_query::QueryResult::Value(_)),
        "ResolveDecl must resolve before the edit"
    );
    let graph = host.project_type_store().semantic_graph();
    assert!(
        graph.get_unvalidated(&key).is_some(),
        "fixture invariant: the warm memo entry must exist after priming"
    );

    // Edit the keyed canonical through the skip-own-drain hook: the
    // own-canonical drain does NOT remove the entry, so the warm read
    // below exercises the entry's self-root validation directly.
    upsert_skip_drain(&host, c, "export type Foo = { a: string; b: number };\n");

    let ctx: &dyn ResolverContext = &host;
    assert!(
        graph.get_unvalidated(&key).is_some(),
        "the physical memo entry must still be present after the skip-drain edit \
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
    let key = SemanticQueryKey::TypeOf {
        value_root: crate::semantic_query::ValueRootKey {
            scope: ScopeId {
                canonical_id: Arc::from(c),
                local_scope: None,
            },
            name: Arc::from("val"),
        },
    };

    let _ = dispatch.execute(key.clone());
    let graph = host.project_type_store().semantic_graph();
    if graph.get_unvalidated(&key).is_none() {
        // `TypeOf` resolution can decline to publish for some shapes;
        // the discriminator only applies when an entry was warmed.
        return;
    }

    upsert_skip_drain(&host, c, "export const val = { a: 1, b: 2 };\n");

    let ctx: &dyn ResolverContext = &host;
    assert!(
        graph.get_unvalidated(&key).is_some(),
        "the physical TypeOf memo entry must still be present after the skip-drain edit"
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
    let _ = dispatch.execute(decl_key);
    let whole_hash = host
        .ensure_indexed_ready(c)
        .map(|indexed| indexed.whole_hash)
        .expect("declaring file IndexedReady materialises");
    let string_arg = graph.intern_node(SemanticNodeData::Primitive(
        crate::semantic_query::PrimitiveKind::String,
    ));
    let key = SemanticQueryKey::Instantiate {
        base: crate::semantic_query::DeclIdentity {
            canonical_id: Arc::from(c),
            whole_hash,
            decl_name: Arc::from("Box"),
        },
        args: Arc::from(vec![string_arg].into_boxed_slice()),
        body_mode: crate::semantic_query::ProjectionMode::Expanded,
    };

    let primed = dispatch.execute(key.clone());
    assert!(
        matches!(primed, crate::semantic_query::QueryResult::Value(_)),
        "Instantiate must resolve before the edit"
    );
    assert!(
        graph.get_unvalidated(&key).is_some(),
        "fixture invariant: the warm Instantiate memo entry must exist after priming"
    );

    // Edit the declaring file through the skip-own-drain hook.
    upsert_skip_drain(
        &host,
        c,
        "export type Box<T> = { value: T; tag: string };\n",
    );

    let ctx: &dyn ResolverContext = &host;
    assert!(
        graph.get_unvalidated(&key).is_some(),
        "the physical Instantiate memo entry must still be present after the skip-drain edit"
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

    let whole_hash = host
        .ensure_indexed_ready(c)
        .map(|indexed| indexed.whole_hash)
        .expect("owner SFC IndexedReady materialises");
    let arg = graph.intern_node(SemanticNodeData::Primitive(
        crate::semantic_query::PrimitiveKind::String,
    ));
    let key = SemanticQueryKey::ResolveMacroPayload {
        owner: crate::semantic_query::DeclIdentity {
            canonical_id: Arc::from(c),
            whole_hash,
            decl_name: Arc::from("<sfc-script-setup>"),
        },
        macro_index: 0,
        macro_kind: verter_semantic::analysis::AnalyzedMacroKind::DefineProps,
        type_args: Arc::from(vec![arg].into_boxed_slice()),
        mode: crate::semantic_query::ProjectionMode::Expanded,
    };

    let primed = dispatch.execute(key.clone());
    assert!(
        matches!(primed, crate::semantic_query::QueryResult::Value(_)),
        "ResolveMacroPayload must resolve before the edit"
    );
    assert!(
        graph.get_unvalidated(&key).is_some(),
        "fixture invariant: the warm ResolveMacroPayload memo entry must exist after priming"
    );

    // Edit the owning SFC through the skip-own-drain hook.
    upsert_skip_drain(
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

fn legacy_whole_hash(canonical: &str, byte: u8) -> DepSignature {
    Arc::from(
        vec![(
            Arc::<str>::from(canonical),
            DepVersion::WholeHash([byte; 16]),
        )]
        .into_boxed_slice(),
    )
}

/// The producer leads the facts rail with one self-root `FileWholeHash`
/// per observed self-root, pinned to the OBSERVED hash.
#[test]
fn read_set_signature_prepends_observed_self_roots() {
    let observed: Vec<(Arc<str>, [u8; 16])> = vec![(Arc::from("/w/a.ts"), [0x11; 16])];
    let carrier =
        semantic_graph_read_set_signature(&observed, &[], &legacy_whole_hash("/w/a.ts", 0x11))
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
    let result =
        semantic_graph_read_set_signature(&observed, &[], &legacy_whole_hash("/w/a.ts", 0x11));
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
    let result =
        semantic_graph_read_set_signature(&observed, &traced, &legacy_whole_hash("/w/a.ts", 0x11));
    assert!(
        result.is_none(),
        "a traced FileWholeHash for a self-root canonical that disagrees with the observed \
         self-root hash is a torn read — the producer MUST return None",
    );
}

/// A `RouteGeneration` legacy dependency has no authoritative
/// validator — the producer returns `None` (non-cacheable).
#[test]
fn read_set_signature_refuses_route_generation_dependency() {
    let observed: Vec<(Arc<str>, [u8; 16])> = vec![(Arc::from("/w/a.ts"), [0x11; 16])];
    let legacy: DepSignature = Arc::from(
        vec![(Arc::<str>::from("/w/a.ts"), DepVersion::RouteGeneration(3))].into_boxed_slice(),
    );
    let result = semantic_graph_read_set_signature(&observed, &[], &legacy);
    assert!(
        result.is_none(),
        "route generation has no authoritative validating source — an entry rooted on it \
         could not detect a content edit, so the producer MUST return None",
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
    let carrier =
        semantic_graph_read_set_signature(&observed, &traced, &legacy_whole_hash("/w/a.ts", 0x11))
            .expect("carrier builds");
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

/// The producer preserves the legacy `DepSignature` rail verbatim so
/// `ProjectGeneration` stays validated by `validate_dep_signature`.
#[test]
fn read_set_signature_preserves_legacy_rail() {
    let observed: Vec<(Arc<str>, [u8; 16])> = vec![(Arc::from("/w/a.ts"), [0x11; 16])];
    let legacy: DepSignature = Arc::from(
        vec![
            (
                Arc::<str>::from("/w/a.ts"),
                DepVersion::WholeHash([0x11; 16]),
            ),
            (
                Arc::<str>::from("<project>"),
                DepVersion::ProjectGeneration(7),
            ),
        ]
        .into_boxed_slice(),
    );
    let carrier =
        semantic_graph_read_set_signature(&observed, &[], &legacy).expect("carrier builds");
    assert!(
        carrier
            .legacy
            .iter()
            .any(|(_, v)| matches!(v, DepVersion::ProjectGeneration(7))),
        "the legacy rail must be preserved so ProjectGeneration stays validated",
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
    let carrier = ReadSetSignature::new(
        Arc::from(vec![FactVersionRef::FileWholeHash {
            canonical_id: untracked.to_string(),
            hash: [0xCD; 16],
        }]),
        Arc::from(Vec::new().into_boxed_slice()),
    );
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

    let carrier = ReadSetSignature::new(
        Arc::from(vec![FactVersionRef::FileWholeHash {
            canonical_id: c.to_string(),
            hash: whole_hash,
        }]),
        Arc::from(Vec::new().into_boxed_slice()),
    );
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
    let carrier = ReadSetSignature::new(
        Arc::from(vec![FactVersionRef::FileWholeHash {
            canonical_id: untracked.to_string(),
            hash: [0xAB; 16],
        }]),
        Arc::from(Vec::new().into_boxed_slice()),
    );
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
    let carrier = ReadSetSignature::new(
        Arc::from(vec![FactVersionRef::FileWholeHash {
            canonical_id: untracked.to_string(),
            hash: [0xEF; 16],
        }]),
        Arc::from(Vec::new().into_boxed_slice()),
    );
    let self_roots: Arc<[Arc<str>]> = Arc::from(vec![Arc::<str>::from(untracked)]);
    store.insert_relation(
        source,
        target,
        carrier,
        self_roots,
        crate::semantic_query::RelationResult::Assignable {
            bindings: Arc::from(Vec::new().into_boxed_slice()),
        },
    );

    assert!(
        store.get_relation(ctx, source, target).is_none(),
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
    store.insert_relation(
        source,
        target,
        ReadSetSignature::empty(),
        Arc::from(Vec::<Arc<str>>::new()),
        crate::semantic_query::RelationResult::Assignable {
            bindings: Arc::from(Vec::new().into_boxed_slice()),
        },
    );

    let cached = store.get_relation(ctx, source, target);
    assert!(
        matches!(
            cached,
            Some((_, crate::semantic_query::RelationResult::Assignable { .. }))
        ),
        "the relation memo must serve a warm judgement whose carrier validates \
         (got {cached:?})",
    );
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
    let key = SemanticQueryKey::KeyOf { base: prim };
    let _ = dispatch.execute(key.clone());

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
    let _ = SemanticNodeId(0);
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
    let _ = dispatch.execute(SemanticQueryKey::ResolveDecl(resolve_decl_key(
        canonical, "Foo",
    )));
    let whole_hash = host
        .ensure_indexed_ready(canonical)
        .map(|indexed| indexed.whole_hash)
        .expect("file IndexedReady materialises");
    let key = SemanticQueryKey::Instantiate {
        base: crate::semantic_query::DeclIdentity {
            canonical_id: Arc::from(canonical),
            whole_hash,
            decl_name: Arc::from("Foo"),
        },
        args: Arc::from(Vec::new().into_boxed_slice()),
        body_mode: crate::semantic_query::ProjectionMode::Expanded,
    };
    match dispatch.execute(key) {
        crate::semantic_query::QueryResult::Value(node) => node,
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

    let key = SemanticQueryKey::KeyOf { base };
    let _ = dispatch.execute(key.clone());
    if graph.get_unvalidated(&key).is_none() {
        // `KeyOf` over a non-Object base does not publish; the
        // discriminator only applies when an entry was warmed.
        return;
    }

    // Edit the base node's originating file through the skip-own-drain
    // hook so the own-canonical drain does not remove the `KeyOf`
    // entry — the warm read exercises the entry's self-root validation.
    upsert_skip_drain(
        &host,
        c,
        "export type Foo = { a: number; b: string; c: boolean };\n",
    );

    let ctx: &dyn ResolverContext = &host;
    assert!(
        graph.get_unvalidated(&key).is_some(),
        "the physical KeyOf memo entry must still be present after the skip-drain edit"
    );
    assert!(
        graph.get_validated(&key, ctx).is_none(),
        "KeyOf: a content edit to the base node's originating file MUST reject the warm \
         memo entry — self-version-rooting derives a self-root FileWholeHash from the base \
         node's origin scope; a `KeyOf` carrier without that self-root validates the stale \
         entry",
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
        mode: crate::semantic_query::ProjectionMode::Navigate,
    };
    let _ = dispatch.execute(key.clone());
    if graph.get_unvalidated(&key).is_none() {
        return;
    }

    upsert_skip_drain(
        &host,
        c,
        "export type Foo = { a: number; b: string; c: boolean };\n",
    );

    let ctx: &dyn ResolverContext = &host;
    assert!(
        graph.get_unvalidated(&key).is_some(),
        "the physical ProjectPath memo entry must still be present after the skip-drain edit"
    );
    assert!(
        graph.get_validated(&key, ctx).is_none(),
        "ProjectPath: a content edit to the projection base's originating file MUST reject \
         the warm memo entry via the base node's file-derived self-root",
    );
}
