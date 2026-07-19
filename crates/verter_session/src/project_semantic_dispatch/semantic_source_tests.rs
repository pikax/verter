//! Discriminating coverage for the `SemanticTypeSource` → [`HotTypeRef`]
//! raising bridge (`semantic_source.rs`).
//!
//! Each test raises ONE source arm through
//! [`ProjectSemanticDispatch::raise_semantic_type_source_to_hot`] and asserts
//! the produced graph node against an independent oracle:
//!
//! - the Authored decl-body arm must intern the SAME node the direct
//!   `lower_locator` provider produces (one memoized query, not a re-lower);
//! - the Authored macro TYPE-ARGUMENT arm must intern the SAME node the sole
//!   sanctioned producer (`macro_type_arg_hot_ref`) produces, while the raw
//!   locator deref for that position stays a typed refusal — proving the
//!   bridge routes AROUND the memo for exactly that arm;
//! - the Closed leaf arm lowers through the in-scope lowerer (a `Ref` leaf
//!   resolves its reference head, a primitive stays a primitive);
//! - the Synthesized object arm composes a real `Object` surface whose member
//!   values lower through their fact-or-locator positions;
//! - the session-demand replay projects the member path off the macro hot
//!   mirror through the ONE dispatch.

use std::sync::Arc;

use verter_type_expr::facts::{
    ClosedTypeFact, FactOrLocator, LeafTypeFact, ResolvedLocalShape, SemanticTypeSource,
    SynthesizedMemberFact,
};
use verter_type_expr::locators::{
    AuthoredAnchor, AuthoredBodyLocator, LocatorSymbolSpace, MacroPayloadLocator,
    MacroPayloadPosition, SymbolBodyLocator, TypeBodyPathStep, TypeBodySlot,
};
use verter_type_expr::span_origins::{MemberSpansOrigin, SourceSynthetic};

use super::SourceRaiseContext;
use crate::locator_identity::{SessionDemandIdentity, SessionDemandOwner, SessionDemandRoute};
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{
    ProjectionMode, ProjectionReductionContext, QueryResult, SemanticNodeData,
};
use crate::types::{HostConfig, UpsertRequest};
use crate::{CompileErrorPolicy, FileLanguage, VerterHost};

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

fn upsert_ts(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();
}

fn upsert_vue(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
}

const OWNER_ID: &str = "/w/semantic-source/owner.ts";
const OWNER: &str = "\
export type Base = { x: string; y: number };\n\
export type Alias = Base;\n";

const SFC_ID: &str = "/w/semantic-source/App.vue";
const SFC: &str = "\
<script setup lang=\"ts\">\n\
defineProps<{ msg: string; count: number }>();\n\
</script>\n\
<template><div /></template>\n";

fn decl_body_locator(canonical: &str, symbol: &str) -> AuthoredBodyLocator {
    AuthoredBodyLocator::DeclBody(TypeBodySlot {
        anchor: AuthoredAnchor {
            canonical_id: Arc::from(canonical),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from(symbol),
            space: LocatorSymbolSpace::Type,
        },
        path: Arc::from(Vec::<TypeBodyPathStep>::new().into_boxed_slice()),
    })
}

fn navigate_ctx(
    scope: &str,
    scope_owner: verter_type_expr::TopLevelOwnerId,
) -> SourceRaiseContext<'_> {
    SourceRaiseContext {
        scope_canonical_id: scope,
        scope_owner,
        context: ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
        interior_failures: None,
    }
}

/// The 0-based index of the first type-based macro in `canonical`.
fn first_macro_index(host: &VerterHost, canonical: &str) -> usize {
    let indexed = host
        .ensure_indexed_ready(canonical)
        .expect("owner SFC IndexedReady must materialise");
    let script = indexed
        .script_analysis
        .as_ref()
        .expect("owner SFC must carry script_analysis");
    script
        .macros
        .iter()
        .position(|m| m.is_type_based)
        .expect("owner SFC must declare a type-based macro")
}

#[test]
fn authored_decl_body_source_raises_to_the_lower_locator_node() {
    let host = host();
    upsert_ts(&host, OWNER_ID, OWNER);
    let dispatch = ProjectSemanticDispatch::new(&host);

    let locator = decl_body_locator(OWNER_ID, "Base");
    let raised = dispatch
        .raise_semantic_type_source_to_hot(
            &SemanticTypeSource::Authored(locator.clone()),
            navigate_ctx(OWNER_ID, verter_type_expr::TopLevelOwnerId::ordinary_file()),
        )
        .expect("the authored decl-body source must raise");

    // Oracle: the direct memoized locator provider. Same interned node id —
    // the bridge is a ROUTE to the one query, never a second lowering.
    let direct = match dispatch.lower_locator(locator) {
        QueryResult::Value(node) | QueryResult::Recursive(node) => node,
        QueryResult::Error(err) => panic!("direct lower_locator must serve the body: {err:?}"),
    };
    assert_eq!(
        raised.node(),
        direct,
        "bridge-raised node must be the SAME interned node the LowerLocator query serves"
    );

    // Negative: the raised node is a real object surface, not a miss shell.
    let data = crate::project_semantic_dispatch::node_data_for(&host, raised.node());
    assert!(
        matches!(data.as_deref(), Some(SemanticNodeData::Object(_))),
        "Base's authored body must raise to an Object surface, got {data:?}"
    );
}

#[test]
fn authored_macro_type_argument_routes_to_the_sole_hot_mirror_producer() {
    let host = host();
    upsert_vue(&host, SFC_ID, SFC);
    let macro_index = first_macro_index(&host, SFC_ID);
    let dispatch = ProjectSemanticDispatch::new(&host);

    let locator = AuthoredBodyLocator::MacroPayload(MacroPayloadLocator {
        anchor: AuthoredAnchor {
            canonical_id: Arc::from(SFC_ID),
            owner: verter_type_expr::TopLevelOwnerId::instance(0),
            symbol: Arc::from("default"),
            space: LocatorSymbolSpace::Value,
        },
        macro_index: u32::try_from(macro_index).unwrap(),
        payload: MacroPayloadPosition::TypeArgument,
    });

    // The raw locator deref for the TYPE-ARGUMENT position is a typed refusal
    // (sole-producer rule) — so a bridge success PROVES the bridge routed
    // through the hot mirror, not the memo.
    assert!(
        matches!(
            dispatch.lower_locator(locator.clone()),
            QueryResult::Error(_)
        ),
        "the macro type-argument locator must stay deref-refused (sole hot-mirror producer)"
    );

    let raised = dispatch
        .raise_semantic_type_source_to_hot(
            &SemanticTypeSource::Authored(locator),
            navigate_ctx(SFC_ID, verter_type_expr::TopLevelOwnerId::instance(0)),
        )
        .expect("the macro type-argument source must raise through the hot mirror");

    let mirror =
        crate::structural_carrier_producer::macro_type_arg_hot_ref(&host, SFC_ID, macro_index)
            .expect("the hot mirror must produce the macro type-arg handle");
    assert_eq!(
        raised.node(),
        mirror.node(),
        "bridge-raised macro type-arg must be the SAME node the sole producer mints"
    );
}

#[test]
fn closed_leaf_sources_lower_in_scope() {
    let host = host();
    upsert_ts(&host, OWNER_ID, OWNER);
    let dispatch = ProjectSemanticDispatch::new(&host);

    // A primitive leaf lowers to the primitive node.
    let primitive = dispatch
        .raise_semantic_type_source_to_hot(
            &SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Primitive(
                verter_type_expr::PrimitiveName::String,
            ))),
            navigate_ctx(OWNER_ID, verter_type_expr::TopLevelOwnerId::ordinary_file()),
        )
        .expect("a primitive leaf must raise");
    assert!(
        matches!(
            crate::project_semantic_dispatch::node_data_for(&host, primitive.node()).as_deref(),
            Some(SemanticNodeData::Primitive(_))
        ),
        "a primitive leaf must lower to a Primitive node"
    );

    // A bare Ref leaf resolves its reference head under the scope's name
    // resolution — a reference carrier, NOT a primitive and NOT a miss.
    let reference = dispatch
        .raise_semantic_type_source_to_hot(
            &SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Ref(
                "Base".to_string(),
            ))),
            navigate_ctx(OWNER_ID, verter_type_expr::TopLevelOwnerId::ordinary_file()),
        )
        .expect("a bare Ref leaf must raise");
    let data = crate::project_semantic_dispatch::node_data_for(&host, reference.node());
    assert!(
        matches!(
            data.as_deref(),
            Some(SemanticNodeData::DeclRef { .. }) | Some(SemanticNodeData::Object(_))
        ),
        "a Ref leaf must resolve its head in scope (DeclRef carrier or resolved body), got {data:?}"
    );
    assert!(
        !matches!(data.as_deref(), Some(SemanticNodeData::Opaque(_))),
        "a resolvable Ref leaf must never raise to a miss shell"
    );
}

fn owner_isolation_fixture() -> VerterHost {
    let host = host();
    upsert_ts(
        &host,
        "/w/semantic-source/module-target.ts",
        "export type ModuleTarget = { moduleOnly: string };\n",
    );
    upsert_ts(
        &host,
        "/w/semantic-source/instance-target.ts",
        "export type InstanceTarget = { instanceOnly: number };\n",
    );
    upsert_vue(
        &host,
        "/w/semantic-source/OwnerIsolation.vue",
        r#"<script lang="ts">
import type { ModuleTarget as Shared } from './module-target'
export type ModuleUse = Shared
</script>
<script setup lang="ts">
import type { InstanceTarget as Shared } from './instance-target'
defineProps<{ value?: Shared }>()
</script>
<template><div /></template>
"#,
    );
    host
}

fn assert_instance_target(host: &VerterHost, node: crate::semantic_query::SemanticNodeId) {
    let data = crate::project_semantic_dispatch::node_data_for(host, node);
    let Some(SemanticNodeData::DeclRef { identity }) = data.as_deref() else {
        panic!("owner-exact reference must lower to a declaration identity, got {data:?}");
    };
    assert_eq!(
        identity.canonical_id.as_ref(),
        "/w/semantic-source/instance-target.ts"
    );
    assert_eq!(identity.decl_name.as_ref(), "InstanceTarget");
}

#[test]
fn closed_ref_leaf_uses_exact_instance_owner_when_module_import_has_same_name() {
    const OWNER: &str = "/w/semantic-source/OwnerIsolation.vue";
    let host = owner_isolation_fixture();
    let dispatch = ProjectSemanticDispatch::new(&host);

    let raised = dispatch
        .raise_semantic_type_source_to_hot(
            &SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Ref(
                "Shared".to_string(),
            ))),
            navigate_ctx(OWNER, verter_type_expr::TopLevelOwnerId::instance(0)),
        )
        .expect("the instance-owner leaf reference must raise");

    assert_instance_target(&host, raised.node());
}

#[test]
fn synthesized_symbol_ref_uses_exact_anchor_owner_when_module_import_has_same_name() {
    const OWNER: &str = "/w/semantic-source/OwnerIsolation.vue";
    let host = owner_isolation_fixture();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let source = SemanticTypeSource::Synthesized(ResolvedLocalShape::Ref(SymbolBodyLocator {
        anchor: AuthoredAnchor {
            canonical_id: Arc::from(OWNER),
            owner: verter_type_expr::TopLevelOwnerId::instance(0),
            symbol: Arc::from("Shared"),
            space: LocatorSymbolSpace::Type,
        },
    }));

    let raised = dispatch
        .raise_semantic_type_source_to_hot(
            &source,
            navigate_ctx(OWNER, verter_type_expr::TopLevelOwnerId::ordinary_file()),
        )
        .expect("the anchor-owner symbol reference must raise");

    assert_instance_target(&host, raised.node());
}

#[test]
fn closed_tuple_leaf_union_element_raises_to_the_ordered_union_node() {
    use verter_type_expr::facts::{TupleElementFact, TuplePayloadFact};
    use verter_type_expr::PrimitiveName;

    let host = host();
    upsert_ts(&host, OWNER_ID, OWNER);
    let dispatch = ProjectSemanticDispatch::new(&host);

    // The realized emit-payload shape: `[payload: string | number]` as a
    // closed tuple whose single element carries the nested leaf-union fact.
    let source = SemanticTypeSource::Closed(ClosedTypeFact::Tuple(TuplePayloadFact {
        readonly: false,
        elements: Arc::from(
            vec![TupleElementFact {
                label: Some("payload".to_string()),
                optional: false,
                rest: false,
                ty: FactOrLocator::LeafUnion(Arc::from(
                    vec![
                        LeafTypeFact::Primitive(PrimitiveName::String),
                        LeafTypeFact::Primitive(PrimitiveName::Number),
                    ]
                    .into_boxed_slice(),
                )),
            }]
            .into_boxed_slice(),
        ),
    }));

    let raised = dispatch
        .raise_semantic_type_source_to_hot(
            &source,
            navigate_ctx(OWNER_ID, verter_type_expr::TopLevelOwnerId::ordinary_file()),
        )
        .expect("a closed tuple with a leaf-union element must raise");

    let data = crate::project_semantic_dispatch::node_data_for(&host, raised.node());
    let Some(SemanticNodeData::Tuple { elements, readonly }) = data.as_deref() else {
        panic!("the closed tuple must compose a Tuple node, got {data:?}");
    };
    assert!(!readonly, "the tuple readonly flag must survive");
    assert_eq!(elements.len(), 1, "the single payload element must survive");
    assert_eq!(
        elements[0].label.as_deref(),
        Some("payload"),
        "the element label must survive"
    );
    assert!(!elements[0].optional);
    assert!(!elements[0].rest);

    // The leaf-union element interns the ORDERED Union node whose members
    // are the lowered leaves — string THEN number, exactly as produced.
    let union_node = elements[0].value;
    let union_data = crate::project_semantic_dispatch::node_data_for(&host, union_node);
    let Some(SemanticNodeData::Union(members)) = union_data.as_deref() else {
        panic!("the leaf-union element must intern a Union node, got {union_data:?}");
    };
    assert_eq!(members.len(), 2, "both union arms must survive");
    assert_eq!(
        dispatch.node_leaf_fact(members[0]),
        Some(LeafTypeFact::Primitive(PrimitiveName::String)),
        "the first union arm must lower to the String primitive"
    );
    assert_eq!(
        dispatch.node_leaf_fact(members[1]),
        Some(LeafTypeFact::Primitive(PrimitiveName::Number)),
        "the second union arm must lower to the Number primitive"
    );

    // The shared node→closed-fact projections round-trip the raised union:
    // the dispatch-owned leaf-union projection recovers the ORDERED leaves,
    // and the element projection wraps them in the nested carrier arm.
    assert_eq!(
        dispatch.node_leaf_union_fact(union_node).as_deref(),
        Some(
            [
                LeafTypeFact::Primitive(PrimitiveName::String),
                LeafTypeFact::Primitive(PrimitiveName::Number),
            ]
            .as_slice()
        ),
        "the dispatch-owned projection must recover the ordered leaves"
    );
    assert!(
        matches!(
            dispatch.node_leaf_fact_or_union(union_node),
            Some(FactOrLocator::LeafUnion(_))
        ),
        "the element projection must mint the nested leaf-union carrier"
    );
    // Negatives: a non-union node projects NO leaf-union fact, and the
    // TUPLE node itself has no complete element fact (richer shapes fail
    // the composite closed instead of publishing a partial fact).
    assert_eq!(dispatch.node_leaf_union_fact(members[0]), None);
    assert_eq!(dispatch.node_leaf_fact_or_union(raised.node()), None);
}

#[test]
fn synthesized_object_source_composes_a_surface_with_lowered_member_values() {
    let host = host();
    upsert_ts(&host, OWNER_ID, OWNER);
    let dispatch = ProjectSemanticDispatch::new(&host);

    let shape = ResolvedLocalShape::Object(Arc::from(
        vec![
            SynthesizedMemberFact {
                name: "flag".to_string(),
                optional: false,
                ty: FactOrLocator::Leaf(LeafTypeFact::Primitive(
                    verter_type_expr::PrimitiveName::Boolean,
                )),
                span_origin: MemberSpansOrigin::Synthetic(SourceSynthetic),
            },
            SynthesizedMemberFact {
                name: "base".to_string(),
                optional: true,
                ty: FactOrLocator::Locator(TypeBodySlot {
                    anchor: AuthoredAnchor {
                        canonical_id: Arc::from(OWNER_ID),
                        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                        symbol: Arc::from("Base"),
                        space: LocatorSymbolSpace::Type,
                    },
                    path: Arc::from(Vec::<TypeBodyPathStep>::new().into_boxed_slice()),
                }),
                span_origin: MemberSpansOrigin::Synthetic(SourceSynthetic),
            },
        ]
        .into_boxed_slice(),
    ));

    let raised = dispatch
        .raise_semantic_type_source_to_hot(
            &SemanticTypeSource::Synthesized(shape),
            navigate_ctx(OWNER_ID, verter_type_expr::TopLevelOwnerId::ordinary_file()),
        )
        .expect("a synthesized object shape must raise");
    let data = crate::project_semantic_dispatch::node_data_for(&host, raised.node());
    let Some(SemanticNodeData::Object(surface)) = data.as_deref() else {
        panic!("a synthesized object must compose an Object node, got {data:?}");
    };
    assert_eq!(surface.members.len(), 2, "both members must survive");

    let flag = surface
        .members
        .iter()
        .find(|m| m.name.as_ref() == "flag")
        .expect("the leaf member must be present");
    assert!(
        matches!(
            crate::project_semantic_dispatch::node_data_for(&host, flag.value).as_deref(),
            Some(SemanticNodeData::Primitive(_))
        ),
        "the leaf member value must lower to a primitive"
    );
    assert!(!flag.optional, "the leaf member is required");

    let base = surface
        .members
        .iter()
        .find(|m| m.name.as_ref() == "base")
        .expect("the locator member must be present");
    assert!(base.optional, "the locator member carries its optionality");
    assert!(
        matches!(
            crate::project_semantic_dispatch::node_data_for(&host, base.value).as_deref(),
            Some(SemanticNodeData::Object(_))
        ),
        "the locator member value must lower through the LowerLocator query to Base's body"
    );
}

#[test]
fn session_demand_replay_projects_the_member_path_off_the_macro_mirror() {
    let host = host();
    upsert_vue(&host, SFC_ID, SFC);
    let macro_index = first_macro_index(&host, SFC_ID);
    let dispatch = ProjectSemanticDispatch::new(&host);

    let demand = SessionDemandIdentity {
        owner: SessionDemandOwner {
            canonical: Arc::from(SFC_ID),
            surface_anchor: Arc::from(macro_index.to_string().as_str()),
        },
        member_role_path: Arc::from(vec!["msg".to_string()].into_boxed_slice()),
        route: SessionDemandRoute::ProjectPath,
    };
    let raised = dispatch
        .replay_session_demand_to_hot(
            &demand,
            ProjectionReductionContext::published(ProjectionMode::Expanded),
        )
        .expect("the session demand must replay to the member node");
    assert!(
        matches!(
            crate::project_semantic_dispatch::node_data_for(&host, raised.node()).as_deref(),
            Some(SemanticNodeData::Primitive(_))
        ),
        "`msg` must project to its primitive member value through the one dispatch"
    );

    // Negative: a demand naming a member the surface does not carry must not
    // fabricate a node.
    let missing = SessionDemandIdentity {
        owner: SessionDemandOwner {
            canonical: Arc::from(SFC_ID),
            surface_anchor: Arc::from(macro_index.to_string().as_str()),
        },
        member_role_path: Arc::from(vec!["not_a_member".to_string()].into_boxed_slice()),
        route: SessionDemandRoute::ProjectPath,
    };
    let replayed = dispatch.replay_session_demand_to_hot(
        &missing,
        ProjectionReductionContext::published(ProjectionMode::Expanded),
    );
    let fabricated_concrete = replayed.is_some_and(|handle| {
        matches!(
            crate::project_semantic_dispatch::node_data_for(&host, handle.node()).as_deref(),
            Some(SemanticNodeData::Primitive(_)) | Some(SemanticNodeData::Literal(_))
        )
    });
    assert!(
        !fabricated_concrete,
        "a missing member must never replay to a fabricated concrete value"
    );
}

/// The synthetic slot-binding DEEPEN route
/// (`SemanticTypeSource::SyntheticSlotBinding` → `deepen_synthetic_binding_to_hot`
/// under an `Expanded` / `Identity` demand) admits into the shared
/// [`crate::component_meta_caches::ShapeCacheDb`] under
/// `ShapeCacheKey::synthetic_binding_whole_with_context`. Its cold reduce
/// (`raise_and_reduce_with_context`) resolves the seed's carrier head through the
/// shared resolver's `ensure_indexed_ready_serve` — so a FENCED (ReturnOnly,
/// `store_published == false`) serve derives the deepened shape from a
/// served-without-publication basis while the entry's fact signature validates
/// against the LIVE view.
///
/// A fenced serve is non-cacheable but NOT partial, so the route's
/// `result_is_partial()`-only admission gate cannot reject it. The admission is
/// only fail-closed if the WHOLE deepen compute runs inside a cacheability tracer
/// whose verdict the admission consults.
///
/// DISCRIMINATING: the unfenced control ADMITS (the slot peeks warm — so the
/// fenced assertion is not vacuous); the fenced run must NOT admit, while the
/// deepened handle still answers the caller (fail-closed refuses the cache write,
/// never the value) and the request stays `Complete`.
#[test]
fn fenced_serve_synthetic_binding_deepen_is_not_admitted() {
    use crate::component_meta_caches::ShapeCacheKey;
    use crate::request_context::{RequestContext, RequestContextGuard};
    use crate::semantic_query::{NodeScopeId, SemanticNodeId, SyntheticBindingId};
    use std::sync::atomic::Ordering;
    use verter_type_expr::{SyntheticCarrierKey, SyntheticCarrierSurfaceKind};

    const SCOPE: &str = "/binding_scope.ts";

    /// Intern a bare-`Ref` seed node in `SCOPE` whose live whole-hash matches the
    /// scope it was minted under (so the deepen's same-generation seed gate opens)
    /// and whose reduce resolves the reference head through
    /// `ensure_indexed_ready_serve` (so the fence has a serve to catch).
    fn intern_seed(host: &VerterHost) -> SemanticNodeId {
        let ctx: &dyn crate::resolver_core::ResolverContext = host;
        let whole_hash = ctx
            .get_whole_hash(SCOPE)
            .expect("the seed scope must have a live whole hash");
        ctx.project_type_store()
            .semantic_graph()
            .intern_node_with_scope(
                SemanticNodeData::new_bare_ref(
                    Arc::from("Anchor"),
                    NodeScopeId::File {
                        canonical_id: Arc::from(SCOPE),
                        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                        whole_hash,
                        local_scope: None,
                    },
                    Arc::from(Vec::new().into_boxed_slice()),
                ),
                NodeScopeId::File {
                    canonical_id: Arc::from(SCOPE),
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    whole_hash,
                    local_scope: None,
                },
            )
    }

    fn carrier_for(seed: SemanticNodeId) -> SyntheticCarrierKey {
        SyntheticCarrierKey {
            scope_canonical_id: Arc::from(SCOPE),
            surface_kind: SyntheticCarrierSurfaceKind::SlotBinding,
            slot_name: Some(Arc::from("default")),
            binding_name: Arc::from("items"),
            value_node: seed.0,
        }
    }

    fn slot_warm(host: &VerterHost, carrier: &SyntheticCarrierKey) -> bool {
        let ctx: &dyn crate::resolver_core::ResolverContext = host;
        let key = ShapeCacheKey::synthetic_binding_whole_with_context(
            SyntheticBindingId::from_carrier_key(carrier),
            ProjectionReductionContext::published(ProjectionMode::Expanded),
        );
        ctx.project_type_store()
            .shape_cache_db()
            .peek(&key, ctx)
            .is_some()
    }

    /// Raise the synthetic-binding source under an `Expanded` demand — the deepen
    /// route (a `Navigate` demand would intern the shallow carrier instead).
    fn drive(host: &VerterHost, carrier: &SyntheticCarrierKey) -> bool {
        let ctx: &dyn crate::resolver_core::ResolverContext = host;
        let dispatch = ProjectSemanticDispatch::new(ctx);
        dispatch
            .raise_semantic_type_source_to_hot(
                &SemanticTypeSource::SyntheticSlotBinding(Arc::new(carrier.clone())),
                SourceRaiseContext {
                    scope_canonical_id: SCOPE,
                    scope_owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    context: ProjectionReductionContext::published(ProjectionMode::Expanded),
                    interior_failures: None,
                },
            )
            .is_some()
    }

    fn fixture() -> VerterHost {
        let host = host();
        upsert_ts(&host, SCOPE, "export type Anchor = { x: number }\n");
        host
    }

    // Control — an UNFENCED deepen admits its reduced shape.
    let control = fixture();
    let control_carrier = carrier_for(intern_seed(&control));
    assert!(
        drive(&control, &control_carrier),
        "fixture invariant: the deepen raise answers the caller",
    );
    assert!(
        slot_warm(&control, &control_carrier),
        "fixture invariant: an unfenced synthetic-binding deepen ADMITS its reduced shape \
         into ShapeCacheDb (otherwise the fenced assertion is vacuous)",
    );

    // Fenced — every `ensure_indexed_ready_serve` the deepen's reduce drives is
    // fenced at a STABLE generation (no bump, so a GenerationSuperseded gate cannot
    // mask the refusal).
    let fenced = fixture();
    let fenced_carrier = carrier_for(intern_seed(&fenced));
    {
        let rctx = RequestContext::new(1, Arc::from(SCOPE), false, None);
        let _guard = RequestContextGuard::install(rctx);
        fenced
            .test_force
            .force_indexed_ready_serve_fence_for_tests
            .store(true, Ordering::Relaxed);
        assert!(
            drive(&fenced, &fenced_carrier),
            "fail-closed refuses the CACHE WRITE, never the value: the fenced deepen still \
             answers the caller",
        );
        fenced
            .test_force
            .force_indexed_ready_serve_fence_for_tests
            .store(false, Ordering::Relaxed);
        assert!(
            !crate::request_context::current_request_result_is_partial(),
            "a fenced serve is non-cacheable, NOT partial — the deepened shape stays Complete",
        );
    }
    assert!(
        !slot_warm(&fenced, &fenced_carrier),
        "POISON: a fenced (non-cacheable) synthetic-binding deepen admitted its shape into \
         ShapeCacheDb. This producer's cold reduce runs with NO cacheability tracer at all, \
         so its `result_is_partial()`-only gate cannot see the fenced serve — the WHOLE \
         compute must run inside a cacheability tracer whose verdict gates the admission",
    );
}
