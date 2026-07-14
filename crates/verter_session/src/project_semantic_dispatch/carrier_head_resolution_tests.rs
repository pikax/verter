//! Demand-time HEAD resolution of `BareRef` / `ImportType` carriers driven
//! through the ONE query-time dispatch.
//!
//! A `BareRef` / `ImportType` carrier names an UNRESOLVED reference head
//! (`Foo` / `Foo<Arg>` / `import("m").G<Arg>`). When such a carrier is a query
//! SUBJECT, the canonical dispatch entry resolves the head through the SAME
//! shared resolver the eager `TypeExpr::Ref` / `TypeExpr::ImportType` lowering
//! path uses (`resolve_bare_ref_head` / `resolve_import_type_head`), rewrites
//! the subject to the resolved node, and dispatches the real semantic subject.
//!
//! These fixtures CONSTRUCT a carrier directly through the sanctioned
//! `SemanticNodeData::new_bare_ref` / `new_import_type` constructors (the
//! carrier's already-lowered `type_args` are carried IN at construction) and
//! drive it through the dispatch as the base of an empty-path `ProjectPath`
//! query. The resolved node is asserted to MATCH the node produced by lowering
//! the equivalent `TypeExpr::Ref` / `TypeExpr::ImportType` through
//! `shallow_lower_type_expr_with_context` — the shared helper gives identical
//! results from BOTH entry points (path-independence).
//!
//! Discrimination: BEFORE head resolution exists, the empty-path terminal
//! returns the bare carrier verbatim (the `expand_terminal_step` pass-through
//! arm), so every "resolves to Foo's body / not the bare carrier" assertion
//! FAILS. AFTER, the subject is resolved before the walk sees it. NEGATIVE
//! assertions throughout (not the bare carrier, not an honest `Opaque(Miss)`,
//! not the still-generic shell where an instantiation was demanded).

use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_semantic::analysis::type_solver::host::ResolvedRootIdentity;
use verter_type_expr::TypeExpr;

use super::carrier::CarrierResolverContext;
use super::ProjectSemanticDispatch;
use crate::resolver_core::scope_shadowing::ScopeShadowing;
use crate::semantic_query::{
    NodeScopeId, PathSegment, PrimitiveKind, ProjectionMode, ProjectionReductionContext,
    QueryResult, SemanticNodeData, SemanticNodeId, SemanticQueryApi, SemanticQueryKey,
    SemanticQueryOutput,
};
use crate::types::HostConfig;
use crate::{CompileErrorPolicy, FileLanguage, UpsertRequest, VerterHost};

pub(super) fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

pub(super) fn upsert_ts(host: &VerterHost, id: &str, source: &str) {
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

/// The `NodeScopeId::File` for `canonical`, sourced from the host's live
/// shallow state (so the `whole_hash` matches the eager lowering path's scope).
pub(super) fn file_scope(dispatch: &ProjectSemanticDispatch<'_>, canonical: &str) -> NodeScopeId {
    let shallow = dispatch
        .ctx
        .shallow_file_state(canonical)
        .expect("file must be indexed in the hermetic host");
    NodeScopeId::File {
        canonical_id: Arc::from(canonical),
        whole_hash: shallow.whole_hash,
        local_scope: None,
    }
}

/// Construct a `BareRef(name, scope, args)` carrier node, where `args` are the
/// interned primitive type arguments (`Foo<string, …>`).
pub(super) fn bare_ref_carrier(
    dispatch: &ProjectSemanticDispatch<'_>,
    name: &str,
    scope: NodeScopeId,
    args: &[PrimitiveKind],
) -> SemanticNodeId {
    let graph = dispatch.graph();
    let arg_nodes: Vec<SemanticNodeId> = args
        .iter()
        .map(|k| graph.intern_node_with_scope(SemanticNodeData::Primitive(*k), scope.clone()))
        .collect();
    graph.intern_node_with_scope(
        SemanticNodeData::new_bare_ref(
            Arc::from(name),
            scope.clone(),
            Arc::from(arg_nodes.into_boxed_slice()),
        ),
        scope,
    )
}

/// Construct an `ImportType(specifier, qualifier, args, typeof_query)` carrier
/// interned UNDER the owner file's scope. An `ImportType` carrier has no scope
/// field; the owner canonical (needed to resolve the relative specifier) is the
/// node-level scope — the structural lowerer interns it with the lowering
/// file's scope, so the head resolver reads `node_scope` to recover the owner.
pub(super) fn import_type_carrier(
    dispatch: &ProjectSemanticDispatch<'_>,
    specifier: &str,
    qualifier: &[&str],
    args: &[PrimitiveKind],
    typeof_query: bool,
    owner_scope: NodeScopeId,
) -> SemanticNodeId {
    let graph = dispatch.graph();
    let qualifier: Arc<[Arc<str>]> = Arc::from(
        qualifier
            .iter()
            .map(|s| Arc::<str>::from(*s))
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    );
    let arg_nodes: Vec<SemanticNodeId> = args
        .iter()
        .map(|k| graph.intern_node_with_scope(SemanticNodeData::Primitive(*k), owner_scope.clone()))
        .collect();
    graph.intern_node_with_scope(
        SemanticNodeData::new_import_type(
            Arc::from(specifier),
            qualifier,
            Arc::from(arg_nodes.into_boxed_slice()),
            typeof_query,
        ),
        owner_scope,
    )
}

/// Drive a carrier node through the dispatch as the base of an empty-path
/// `ProjectPath` query in `mode`, returning the resolved subject node.
fn resolve_subject(
    dispatch: &ProjectSemanticDispatch<'_>,
    carrier: SemanticNodeId,
    mode: ProjectionMode,
) -> SemanticNodeId {
    let empty_path: Arc<[PathSegment]> = Arc::from(Vec::new().into_boxed_slice());
    match dispatch.execute_type_node(SemanticQueryKey::ProjectPath {
        base: carrier,
        path: empty_path,
        context: ProjectionReductionContext::published(mode),
    }) {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        // A `Recursive` result (the bounded recursive-ref back-edge) carries a
        // node id too — a valid BOUNDED termination, not a failure.
        QueryResult::Recursive(id) => id,
        other => panic!("ProjectPath over a carrier subject did not yield a node: {other:?}"),
    }
}

/// Lower the equivalent `TypeExpr::Ref` (or `ImportType`) through the eager
/// path in `mode` with an EMPTY `name_resolution` + a scope-derived
/// payload/shadowing — the SAME shape `lower_type_expr_in_scope_with_context`
/// uses in production. Returns the eager-lowered node (NOT pre-projected), so
/// the caller compares at the SAME stage as the carrier-resolved subject.
fn eager_lower_subject(
    dispatch: &ProjectSemanticDispatch<'_>,
    expr: &TypeExpr,
    canonical: &str,
    mode: ProjectionMode,
) -> SemanticNodeId {
    let scope = file_scope(dispatch, canonical);
    let env: FxHashMap<String, SemanticNodeId> = FxHashMap::default();
    let name_resolution: FxHashMap<String, ResolvedRootIdentity> = FxHashMap::default();
    let scope_payload = dispatch.ctx.prepared_decl_bundle(canonical).map(|bundle| {
        crate::resolver_core::bare_name_resolve::DeclarationScopePayload::from_bundle(&bundle)
    });
    let shadowing = ScopeShadowing::from_scope_payload(scope_payload.as_ref());
    let mut substitutions: Vec<(Arc<str>, SemanticNodeId)> = Vec::new();
    dispatch.shallow_lower_type_expr_with_context(
        expr,
        &env,
        &scope,
        &name_resolution,
        scope_payload.as_ref(),
        &shadowing,
        &mut substitutions,
        ProjectionReductionContext::published(mode),
    )
}

/// The eager-lowered node projected through the SAME empty-path `ProjectPath`
/// the carrier-subject path runs — so the two entry points are compared at the
/// SAME projection stage (path-independence).
fn eager_resolved(
    dispatch: &ProjectSemanticDispatch<'_>,
    expr: &TypeExpr,
    canonical: &str,
    mode: ProjectionMode,
) -> SemanticNodeId {
    let lowered = eager_lower_subject(dispatch, expr, canonical, mode);
    resolve_subject(dispatch, lowered, mode)
}

fn is_opaque(dispatch: &ProjectSemanticDispatch<'_>, node: SemanticNodeId) -> bool {
    matches!(
        dispatch.graph().node_data(node).as_deref(),
        Some(SemanticNodeData::Opaque(_))
    )
}

fn is_bare_ref(dispatch: &ProjectSemanticDispatch<'_>, node: SemanticNodeId) -> bool {
    matches!(
        dispatch.graph().node_data(node).as_deref(),
        Some(SemanticNodeData::BareRef(_))
    )
}

fn is_import_type(dispatch: &ProjectSemanticDispatch<'_>, node: SemanticNodeId) -> bool {
    matches!(
        dispatch.graph().node_data(node).as_deref(),
        Some(SemanticNodeData::ImportType(_))
    )
}

fn is_decl_ref(dispatch: &ProjectSemanticDispatch<'_>, node: SemanticNodeId) -> bool {
    matches!(
        dispatch.graph().node_data(node).as_deref(),
        Some(SemanticNodeData::DeclRef { .. })
    )
}

/// `Some((member_name, primitive))` for the first member of the Object the
/// `subject` (a carrier OR a resolved node) materializes to under EXPANDED
/// mode. The carrier-subject entry hook resolves a carrier in the query's
/// (Expanded) mode — which routes a resolved head through `Instantiate` to the
/// Object body — so this drives the subject in Expanded and reads the Object.
fn first_member_primitive(
    dispatch: &ProjectSemanticDispatch<'_>,
    subject: SemanticNodeId,
) -> Option<(String, PrimitiveKind)> {
    let expanded = resolve_subject(dispatch, subject, ProjectionMode::Expanded);
    first_member_primitive_of_object(dispatch, expanded)
}

/// `Some((member_name, primitive))` for the first member of `node` IF `node` is
/// directly an Object with a primitive first-member value. No re-projection.
fn first_member_primitive_of_object(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> Option<(String, PrimitiveKind)> {
    let data = dispatch.graph().node_data(node)?;
    let SemanticNodeData::Object(view) = data.as_ref() else {
        return None;
    };
    let member = view.members.first()?;
    let value = dispatch.graph().node_data(member.value)?;
    match value.as_ref() {
        SemanticNodeData::Primitive(k) => Some((member.name.as_ref().to_string(), *k)),
        _ => None,
    }
}

// ── B1. Workspace-owned bare name ───────────────────────────────────────────
//
// `BareRef("Foo")` over a file declaring `type Foo = { a: string }` resolves to
// Foo's declaration body (a `DeclRef`/resolved node) — NOT the bare carrier,
// NOT an `Opaque(Miss)`.
#[test]
fn bare_ref_head_resolves_workspace_owned_name() {
    let host = host();
    upsert_ts(&host, "/m.ts", "export type Foo = { a: string };\n");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let scope = file_scope(&dispatch, "/m.ts");

    let carrier = bare_ref_carrier(&dispatch, "Foo", scope, &[]);
    let resolved = resolve_subject(&dispatch, carrier, ProjectionMode::Navigate);

    assert!(
        !is_bare_ref(&dispatch, resolved),
        "the BareRef head must be RESOLVED, not left as the bare carrier: {:?}",
        dispatch.graph().node_data(resolved).as_deref()
    );
    assert!(
        !is_opaque(&dispatch, resolved),
        "`Foo` is a workspace-owned type and must resolve, not miss"
    );
    // Driving the carrier in Expanded mode materializes Foo's body
    // `{ a: string }` (a Navigate `DeclRef` is not unwrapped by the empty-path
    // terminal, by design — the Expanded route runs `Instantiate`).
    let (member, kind) = first_member_primitive(&dispatch, carrier)
        .expect("resolved `Foo` must expand to its declared object body");
    assert_eq!(member, "a");
    assert_eq!(kind, PrimitiveKind::String);
}

// ── B2. Bare name + args (instantiation) ────────────────────────────────────
//
// `BareRef("Box", [string])` over `type Box<T> = { v: T }` resolves+instantiates
// to `{ v: string }` — the carrier's already-lowered `type_args` reach the
// instantiation. NEG: not the un-instantiated generic, not a miss, not the bare
// carrier.
#[test]
fn bare_ref_head_resolves_and_instantiates_args() {
    let host = host();
    upsert_ts(&host, "/box.ts", "export type Box<T> = { v: T };\n");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let scope = file_scope(&dispatch, "/box.ts");

    let carrier = bare_ref_carrier(&dispatch, "Box", scope, &[PrimitiveKind::String]);
    let resolved = resolve_subject(&dispatch, carrier, ProjectionMode::Navigate);

    assert!(!is_bare_ref(&dispatch, resolved), "head must be resolved");
    assert!(
        !is_opaque(&dispatch, resolved),
        "`Box<string>` must resolve"
    );

    let (member, kind) = first_member_primitive(&dispatch, carrier)
        .expect("resolved `Box<string>` must expand to `{ v: string }`");
    assert_eq!(member, "v");
    assert_eq!(
        kind,
        PrimitiveKind::String,
        "the carrier's `string` type-arg must substitute into `T`; a dropped-args resolver \
         would leave `v` a free `TypeParam(T)`"
    );
}

// ── B3. ImportType head (type position + typeof_query) ──────────────────────
//
// `ImportType("./dep", ["G"])` in type position resolves to the imported type
// `G`; with a type-arg it instantiates. `typeof import("./dep")` resolves to the
// module's value namespace.
#[test]
fn import_type_head_resolves_type_position() {
    let host = host();
    upsert_ts(&host, "/dep.ts", "export type G<T> = { g: T };\n");
    upsert_ts(&host, "/owner.ts", "export const x = 1;\n");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let scope = file_scope(&dispatch, "/owner.ts");

    // `import("./dep").G` — bare (no args).
    let carrier = import_type_carrier(&dispatch, "./dep", &["G"], &[], false, scope);
    let resolved = resolve_subject(&dispatch, carrier, ProjectionMode::Navigate);
    assert!(
        !is_import_type(&dispatch, resolved),
        "the ImportType head must be resolved, not left as the bare carrier"
    );
    assert!(
        !is_opaque(&dispatch, resolved),
        "`import(\"./dep\").G` resolves to the imported type"
    );
}

#[test]
fn import_type_head_resolves_and_instantiates_args() {
    let host = host();
    upsert_ts(&host, "/dep.ts", "export type G<T> = { g: T };\n");
    upsert_ts(&host, "/owner.ts", "export const x = 1;\n");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let scope = file_scope(&dispatch, "/owner.ts");

    let carrier = import_type_carrier(
        &dispatch,
        "./dep",
        &["G"],
        &[PrimitiveKind::Number],
        false,
        scope,
    );
    let resolved = resolve_subject(&dispatch, carrier, ProjectionMode::Navigate);
    assert!(!is_import_type(&dispatch, resolved), "head must resolve");
    assert!(
        !is_opaque(&dispatch, resolved),
        "`import(\"./dep\").G<number>` resolves"
    );

    let (member, kind) = first_member_primitive(&dispatch, carrier)
        .expect("resolved `import(\"./dep\").G<number>` must expand to `{ g: number }`");
    assert_eq!(member, "g");
    assert_eq!(kind, PrimitiveKind::Number);
}

#[test]
fn import_type_head_resolves_typeof_query() {
    let host = host();
    upsert_ts(&host, "/vals.ts", "export const k = 7;\n");
    upsert_ts(&host, "/owner.ts", "export const x = 1;\n");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let scope = file_scope(&dispatch, "/owner.ts");

    // `typeof import("./vals")` — the module's value-export namespace.
    let carrier = import_type_carrier(&dispatch, "./vals", &[], &[], true, scope);
    let resolved = resolve_subject(&dispatch, carrier, ProjectionMode::Navigate);
    assert!(
        !is_import_type(&dispatch, resolved),
        "the typeof-import head must resolve to the value namespace, not the bare carrier"
    );
    // Equivalence with the eager `TypeExpr::ImportType { typeof_query: true }`.
    let eager = eager_resolved(
        &dispatch,
        &TypeExpr::ImportType {
            specifier: Arc::from("./vals"),
            qualifier: Arc::from(Vec::<Arc<str>>::new().into_boxed_slice()),
            typeof_query: true,
            type_arguments: verter_type_expr::empty_type_args(),
        },
        "/owner.ts",
        ProjectionMode::Navigate,
    );
    assert_eq!(
        resolved, eager,
        "carrier-head `typeof import(\"./vals\")` must resolve identically to the eager lowering"
    );
}

// ── B4. Augmentation (external + relative `declare module`) ──────────────────
//
// A head whose target is augmented by a cross-file `declare module "<spec>"`
// resolves to the peer-merged surface (the augmenter contributions).
#[test]
fn bare_ref_head_resolves_external_module_augmentation() {
    let host = host();
    // The importing file imports `Cfg` from an EXTERNAL bare specifier that
    // resolves to no workspace file; a `declare module "ext-pkg"` augmenter
    // contributes the surface.
    upsert_ts(
        &host,
        "/aug.d.ts",
        "declare module \"ext-pkg\" { export interface Cfg { mode: string } }\n",
    );
    upsert_ts(
        &host,
        "/use.ts",
        "import type { Cfg } from \"ext-pkg\";\nexport type U = Cfg;\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let scope = file_scope(&dispatch, "/use.ts");

    let carrier = bare_ref_carrier(&dispatch, "Cfg", scope, &[]);
    let resolved = resolve_subject(&dispatch, carrier, ProjectionMode::Navigate);

    // Equivalence: the carrier head resolves identically to the eager
    // `TypeExpr::Ref { name: "Cfg" }` in the same scope (both route through the
    // external-augmentation stitch).
    let eager = eager_resolved(
        &dispatch,
        &TypeExpr::Ref {
            name: Arc::from("Cfg"),
            type_arguments: verter_type_expr::empty_type_args(),
        },
        "/use.ts",
        ProjectionMode::Navigate,
    );
    assert_eq!(
        resolved, eager,
        "carrier-head `Cfg` (external `declare module` augmented) must resolve identically to \
         the eager `Ref` lowering"
    );
    assert!(
        !is_bare_ref(&dispatch, resolved),
        "the augmented head must resolve to the merged surface, not the bare carrier"
    );
}

// ── B19. BARREL re-export — CARRIER vs eager-fast-path divergence (LATENT) ─
//
// `import { X } from "./barrel"` where `/barrel.ts` re-exports
// `export { X } from "./real"`. The carrier path's rehydrated
// `resolve_bare_name_in_scope` walks the re-export chain to the FINAL defining
// file `/real.ts` (`DeclRef@/real.ts`), while the eager `name_resolution`
// fast-path stores the IMMEDIATE barrel target (`DeclRef@/barrel.ts`) and relies
// on DOWNSTREAM `ResolveDecl`-through-the-barrel to reach `/real.ts`. Both
// materialise to the same type, but the carrier-MODE published `DeclRef`
// IDENTITY differs.
//
// This is a LATENT divergence: NO production path emits `BareRef` carriers today
// (the structural lowerer is dormant), so it cannot be validated end-to-end
// here. Converging the two would require EITHER making the carrier path stop at
// the barrel (dropping the re-export walk `resolve_bare_name_in_scope` needs for
// the augmentation / cross-owner cases) OR making the eager fast-path walk to
// the final file (changing the PRODUCTION eager path, risking regression on
// exercised code) — neither is a clean carrier-local fix. RECORDED for
// re-validation at the producer flip. This test CHARACTERIZES the current
// (divergent) behavior so a future change is forced to update it deliberately.
#[test]
fn carrier_head_barrel_reexport_walks_to_final_file_eager_stops_at_barrel() {
    let host = host();
    upsert_ts(&host, "/real.ts", "export type X = { x: string };\n");
    upsert_ts(&host, "/barrel.ts", "export { X } from \"./real\";\n");
    upsert_ts(
        &host,
        "/consumer.ts",
        "import type { X } from \"./barrel\";\nexport type Use = X;\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let scope = file_scope(&dispatch, "/consumer.ts");

    let carrier = bare_ref_carrier(&dispatch, "X", scope, &[]);
    let via_carrier = resolve_subject(&dispatch, carrier, ProjectionMode::Navigate);
    assert!(
        !is_bare_ref(&dispatch, via_carrier),
        "barrel head must resolve"
    );
    assert!(!is_opaque(&dispatch, via_carrier), "barrel `X` resolves");

    // CHARACTERIZE: the carrier path reaches the FINAL defining file `/real.ts`.
    let carrier_canonical = match dispatch.graph().node_data(via_carrier).as_deref() {
        Some(SemanticNodeData::DeclRef { identity }) => identity.canonical_id.to_string(),
        other => panic!("carrier barrel head must be a DeclRef; got {other:?}"),
    };
    assert_eq!(
        carrier_canonical, "/real.ts",
        "the carrier path walks the re-export chain to the FINAL defining file (recorded          behavior; eager fast-path stops at the barrel — a LATENT divergence to re-validate          at the producer flip)"
    );

    // CHARACTERIZE the eager fast-path stops at the intermediate barrel — proving
    // the divergence is real (not interning noise).
    let eager = eager_resolved_with_name_resolution(
        &dispatch,
        &TypeExpr::Ref {
            name: Arc::from("X"),
            type_arguments: verter_type_expr::empty_type_args(),
        },
        "/consumer.ts",
        ProjectionMode::Navigate,
    );
    let eager_canonical = match dispatch.graph().node_data(eager).as_deref() {
        Some(SemanticNodeData::DeclRef { identity }) => identity.canonical_id.to_string(),
        other => panic!("eager barrel head must be a DeclRef; got {other:?}"),
    };
    assert_eq!(
        eager_canonical, "/barrel.ts",
        "the eager name_resolution fast-path stores the IMMEDIATE barrel canonical (recorded          behavior). The carrier/eager carrier-mode DeclRef identities differ here — the LATENT          divergence the producer flip must re-validate."
    );
}

// ── B20. NAMESPACE-SIBLING — CARRIER misses where eager fast-path resolves
//        (LATENT, recorded for the producer flip) ─────────────────────────────
//
// Inside `namespace NS { type Sib = ...; type Member = Sib }`, the eager path
// lowering `NS.Member`'s body injects a bare-name `name_resolution` entry
// `"Sib" -> (file, "NS.Sib")` via `add_namespace_sibling_resolutions`. The
// carrier path rehydrates from the SCOPE PAYLOAD (not a specific decl's injected
// `name_resolution`), and `resolve_bare_name_in_scope` cannot reconstruct the
// `NS.`-qualified sibling binding for a BARE `Sib` — so a `BareRef("Sib")`
// resolved against the file scope MISSES (or resolves to a different node) where
// the eager populated fast-path resolves to `NS.Sib`.
//
// This is LATENT: no production path emits `BareRef` carriers, and crucially the
// carrier's enclosing-NAMESPACE context (which decl's body it was lowered in) is
// determined by the DORMANT structural lowerer — without that producer the
// correct carrier scope shape for a namespace-sibling reference is not pinned.
// A clean fix REQUIRES the producer to exercise it end-to-end, so it is RECORDED
// for the producer flip, NOT faked here. This test documents the divergence:
// the carrier-path resolution of a bare sibling name does NOT reach `NS.Sib`.
#[test]
fn carrier_head_namespace_sibling_bare_name_diverges_recorded_for_producer_flip() {
    let host = host();
    upsert_ts(
        &host,
        "/ns.ts",
        "export namespace NS {\n  export type Sib = { s: string };\n  export type Member = Sib;\n}\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let scope = file_scope(&dispatch, "/ns.ts");

    // A bare `BareRef("Sib")` against the FILE scope — the carrier path has no
    // namespace-sibling `name_resolution` injection to reconstruct.
    let carrier = bare_ref_carrier(&dispatch, "Sib", scope, &[]);
    let via_carrier = resolve_subject(&dispatch, carrier, ProjectionMode::Navigate);

    // PIN the EXACT current divergent terminal. The carrier path rehydrates from
    // the SCOPE PAYLOAD only — it has no namespace-sibling `name_resolution`
    // injection — so the bare `Sib` does NOT resolve. Under the Navigate carrier
    // mode an unresolved head PRESERVES the scoped `BareRef` carrier (never
    // `Opaque(Miss)`, which would destroy the authored head), so the divergent
    // terminal is the carrier RETURNED UNCHANGED. Pinning the exact unchanged
    // carrier (not merely "!= NS.Sib") makes this characterization FLIP if the
    // carrier-path terminal CHANGES for any reason — a regression to a different
    // wrong declaration, an opaque, or (the intended event) the producer flip
    // wiring the structural lowerer's carrier-scope shape so the sibling binding
    // resolves. Any of those forces a re-validation here.
    assert_eq!(
        via_carrier,
        carrier,
        "RECORDED namespace-sibling gap: the carrier path is expected to return the \
         `BareRef(\"Sib\")` carrier UNCHANGED (an unresolved head under the Navigate \
         carrier mode preserves the scoped carrier) — it has no scope-payload \
         equivalent of the eager path's per-decl `add_namespace_sibling_resolutions` \
         injection. The terminal CHANGED, so the latent divergence must be \
         re-validated (this is what the producer flip should trigger). Got: {:?}",
        dispatch.graph().node_data(via_carrier).as_deref()
    );
    match dispatch.graph().node_data(via_carrier).as_deref() {
        Some(data)
            if data
                .bare_ref_head()
                .is_some_and(|(n, _)| n.as_ref() == "Sib") => {}
        other => panic!(
            "the preserved divergent terminal must still be the `BareRef(\"Sib\")` \
             carrier; got {other:?}"
        ),
    }

    // CONTRAST: the EAGER path (with the namespace-sibling `name_resolution`
    // entry the per-decl injection would populate — `"Sib" -> (/ns.ts, NS.Sib)`)
    // DOES resolve the bare `Sib` to the member `NS.Sib`. Anchoring the eager
    // resolution here proves the divergence is REAL (the carrier miss above is a
    // genuine gap against a path that succeeds, not interning noise) and pins
    // BOTH sides: this characterization flips if EITHER terminal moves.
    let eager_sib = {
        let eager_scope = file_scope(&dispatch, "/ns.ts");
        let env: FxHashMap<String, SemanticNodeId> = FxHashMap::default();
        let scope_payload = dispatch.ctx.prepared_decl_bundle("/ns.ts").map(|bundle| {
            crate::resolver_core::bare_name_resolve::DeclarationScopePayload::from_bundle(&bundle)
        });
        let mut name_resolution: FxHashMap<String, ResolvedRootIdentity> = FxHashMap::default();
        name_resolution.insert(
            "Sib".to_string(),
            ResolvedRootIdentity::new("/ns.ts", "NS.Sib"),
        );
        let shadowing = ScopeShadowing::from_scope_payload(scope_payload.as_ref());
        let mut substitutions: Vec<(Arc<str>, SemanticNodeId)> = Vec::new();
        let lowered = dispatch.shallow_lower_type_expr_with_context(
            &TypeExpr::Ref {
                name: Arc::from("Sib"),
                type_arguments: verter_type_expr::empty_type_args(),
            },
            &env,
            &eager_scope,
            &name_resolution,
            scope_payload.as_ref(),
            &shadowing,
            &mut substitutions,
            ProjectionReductionContext::published(ProjectionMode::Navigate),
        );
        resolve_subject(&dispatch, lowered, ProjectionMode::Navigate)
    };
    let eager_decl_name = match dispatch.graph().node_data(eager_sib).as_deref() {
        Some(SemanticNodeData::DeclRef { identity }) => identity.decl_name.to_string(),
        other => panic!(
            "the eager namespace-sibling fast-path must resolve the bare `Sib` to a \
             `DeclRef`; got {other:?}"
        ),
    };
    assert_eq!(
        eager_decl_name, "NS.Sib",
        "the eager path (with the sibling `name_resolution` injection) resolves bare `Sib` \
         to the member `NS.Sib` — the carrier `Opaque(Miss)` above is the LATENT divergence"
    );
    assert_ne!(
        dispatch.graph().node_data(via_carrier).as_deref(),
        dispatch.graph().node_data(eager_sib).as_deref(),
        "the carrier `Sib` terminal (`Opaque(Miss)`) must DIFFER from the eager `NS.Sib` \
         resolution — the recorded gap the producer flip must re-validate"
    );
}

// ── B5. Enum-member projection ──────────────────────────────────────────────
//
// `BareRef("Color.Red")` where `Color` is an enum projects the member's value
// type (gated on `ValueDeclKind::Enum`). NEG: not a miss, not the bare carrier.
#[test]
fn bare_ref_head_projects_enum_member() {
    let host = host();
    upsert_ts(
        &host,
        "/enum.ts",
        "export enum Color { Red = \"red\", Blue = \"blue\" }\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let scope = file_scope(&dispatch, "/enum.ts");

    let carrier = bare_ref_carrier(&dispatch, "Color.Red", scope, &[]);
    let resolved = resolve_subject(&dispatch, carrier, ProjectionMode::Navigate);

    // Equivalence with the eager `TypeExpr::Ref { name: "Color.Red" }`.
    let eager = eager_resolved(
        &dispatch,
        &TypeExpr::Ref {
            name: Arc::from("Color.Red"),
            type_arguments: verter_type_expr::empty_type_args(),
        },
        "/enum.ts",
        ProjectionMode::Navigate,
    );
    assert_eq!(
        resolved, eager,
        "carrier-head enum-member `Color.Red` must project identically to the eager `Ref` path"
    );
    assert!(
        !is_bare_ref(&dispatch, resolved),
        "the enum-member head must project the member value, not stay the bare carrier"
    );
    assert!(
        !is_opaque(&dispatch, resolved),
        "`Color.Red` is a declared enum member and must project, not miss"
    );
}

// ── B6. Builtin shadowing (userland alias wins) ─────────────────────────────
//
// `BareRef("Partial")` in a scope that declares `type Partial<T> = { p: T }`
// resolves the USERLAND alias, NOT the builtin `Partial`.
#[test]
fn bare_ref_head_userland_shadow_wins_over_builtin() {
    let host = host();
    upsert_ts(&host, "/shadow.ts", "export type Partial<T> = { p: T };\n");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let scope = file_scope(&dispatch, "/shadow.ts");

    let carrier = bare_ref_carrier(&dispatch, "Partial", scope, &[PrimitiveKind::Boolean]);
    let resolved = resolve_subject(&dispatch, carrier, ProjectionMode::Navigate);

    assert!(!is_bare_ref(&dispatch, resolved), "head must resolve");
    assert!(
        !is_opaque(&dispatch, resolved),
        "userland `Partial<boolean>` resolves"
    );
    // The USERLAND `Partial<T> = { p: T }` body — member `p`, NOT the builtin
    // `Partial`'s mapped-type surface (which would not surface a bare `p`).
    let (member, kind) = first_member_primitive(&dispatch, carrier)
        .expect("resolved userland `Partial<boolean>` must expand to `{ p: boolean }`");
    assert_eq!(
        member, "p",
        "the USERLAND alias must win over the builtin `Partial`"
    );
    assert_eq!(kind, PrimitiveKind::Boolean);
}

// ── B7. Recursion back-edge (worker thread for stack-overflow safety) ───────
//
// A self-referential head resolving to a `(canonical, name)` already on the
// instantiate-active stack mints `Opaque(RecursiveRef)` and terminates bounded.
// The carrier head must consult the SAME `is_instantiate_active` back-edge as
// the eager `Ref` arm. Run on a worker thread so a non-terminating resolver
// (stack overflow) is caught rather than aborting the test process.
#[test]
fn bare_ref_head_recursive_ref_terminates_bounded() {
    let handle = std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            let host = host();
            // A recursive type: `type Tree = { children: Tree }`. Resolving the
            // inner `Tree` head while the outer `Tree` Instantiate is active
            // must back-edge.
            upsert_ts(
                &host,
                "/tree.ts",
                "export type Tree = { children: Tree };\n",
            );
            let dispatch = ProjectSemanticDispatch::new(&host);
            let scope = file_scope(&dispatch, "/tree.ts");

            // Push the `(canonical, name)` identity active to simulate being
            // INSIDE `build_instantiate`'s push/pop window for `Tree`.
            let pushed =
                dispatch.push_instantiate_active((Arc::from("/tree.ts"), Arc::from("Tree")));
            assert!(pushed, "the identity must not already be active");

            let carrier = bare_ref_carrier(&dispatch, "Tree", scope, &[]);
            let resolved = resolve_subject(&dispatch, carrier, ProjectionMode::Navigate);

            dispatch.pop_instantiate_active();

            // The active back-edge mints `Opaque(RecursiveRef { name: "Tree" })`.
            match dispatch.graph().node_data(resolved).as_deref() {
                Some(SemanticNodeData::Opaque(
                    crate::semantic_query::QueryError::RecursiveRef { name },
                )) => {
                    assert_eq!(
                        name.as_ref(),
                        "Tree",
                        "the back-edge must carry the recursive name"
                    );
                }
                other => panic!(
                    "a self-referential head while the identity is active must mint \
                     Opaque(RecursiveRef), got {other:?}"
                ),
            }
        })
        .expect("spawn worker thread");
    handle
        .join()
        .expect("the recursive-head resolution must terminate bounded (no stack overflow)");
}

// ── B8. Nested carrier (Intersection arm) — re-entry, not top-level only ────
//
// A `BareRef` carrier nested inside an `Intersection` member resolves correctly
// when the intersection is the query subject. This proves the walker worklist
// RE-ENTERS the dispatch normalization for a nested carrier (not just the
// top-level subject).
#[test]
fn nested_bare_ref_carrier_in_intersection_resolves() {
    let host = host();
    upsert_ts(
        &host,
        "/n.ts",
        "export type A = { a: string };\nexport type B = { b: number };\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = dispatch.graph();
    let scope = file_scope(&dispatch, "/n.ts");

    // Build `BareRef("A") & BareRef("B")` as an Intersection of two carriers.
    let a = bare_ref_carrier(&dispatch, "A", scope.clone(), &[]);
    let b = bare_ref_carrier(&dispatch, "B", scope, &[]);
    let intersection = graph.intern_node_with_scope(
        SemanticNodeData::Intersection(Arc::from(vec![a, b].into_boxed_slice())),
        NodeScopeId::Global,
    );

    // Expand the intersection surface (Shallow synthesis): both nested carriers
    // must resolve so the merged surface carries `a` AND `b`.
    let surface = resolve_subject(&dispatch, intersection, ProjectionMode::Shallow);
    let data = graph
        .node_data(surface)
        .expect("intersection surface must materialize");
    let SemanticNodeData::Object(view) = data.as_ref() else {
        panic!(
            "nested carriers in an Intersection must synthesize a merged Object surface; got {:?}",
            data.as_ref()
        );
    };
    let member_names: Vec<&str> = view.members.iter().map(|m| m.name.as_ref()).collect();
    assert!(
        member_names.contains(&"a"),
        "nested `BareRef(A)` must resolve so its member `a` surfaces; members: {member_names:?}"
    );
    assert!(
        member_names.contains(&"b"),
        "nested `BareRef(B)` must resolve so its member `b` surfaces; members: {member_names:?}"
    );
}

// ── B9. EQUIVALENCE / path-independence (B1/B2/B3) ──────────────────────────
//
// The carrier-head-resolved node EQUALS the node produced by lowering the
// equivalent `TypeExpr` through the eager path and running the SAME empty-path
// projection. One shared resolver ⇒ identical result from both entry points.
#[test]
fn carrier_head_resolution_is_path_independent_with_eager_lowering() {
    let host = host();
    upsert_ts(
        &host,
        "/eq.ts",
        "export type Foo = { a: string };\nexport type Box<T> = { v: T };\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let scope = file_scope(&dispatch, "/eq.ts");

    // B1 shape: bare `Foo`.
    let carrier_foo = bare_ref_carrier(&dispatch, "Foo", scope.clone(), &[]);
    let via_carrier_foo = resolve_subject(&dispatch, carrier_foo, ProjectionMode::Navigate);
    let via_eager_foo = eager_resolved(
        &dispatch,
        &TypeExpr::Ref {
            name: Arc::from("Foo"),
            type_arguments: verter_type_expr::empty_type_args(),
        },
        "/eq.ts",
        ProjectionMode::Navigate,
    );
    assert_eq!(
        via_carrier_foo, via_eager_foo,
        "carrier-head `Foo` must equal the eager `Ref {{ Foo }}` lowering (path-independence)"
    );

    // B2 shape: `Box<string>`.
    let carrier_box = bare_ref_carrier(&dispatch, "Box", scope, &[PrimitiveKind::String]);
    let via_carrier_box = resolve_subject(&dispatch, carrier_box, ProjectionMode::Navigate);
    let via_eager_box = eager_resolved(
        &dispatch,
        &TypeExpr::Ref {
            name: Arc::from("Box"),
            type_arguments: Arc::from(
                vec![TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)]
                    .into_boxed_slice(),
            ),
        },
        "/eq.ts",
        ProjectionMode::Navigate,
    );
    assert_eq!(
        via_carrier_box, via_eager_box,
        "carrier-head `Box<string>` must equal the eager `Ref {{ Box<string> }}` lowering"
    );
    assert_ne!(
        via_carrier_box, via_carrier_foo,
        "distinct heads must resolve to distinct nodes (no collapse)"
    );
}

// ── B10. name_resolution equivalence / rehydration (finding #3) ─────────────
//
// A bare name that the eager `name_resolution` FAST-PATH would resolve — here a
// cross-file IMPORT binding — must resolve to the SAME target when driven via a
// carrier whose `CarrierResolverContext` is rehydrated from scope (empty
// name_resolution + scope payload). This proves the carrier path did NOT
// silently drop the `name_resolution` semantics: the rehydrated
// `resolve_bare_name_in_scope` fallback recovers the import binding.
#[test]
fn carrier_head_rehydrates_name_resolution_for_import_binding() {
    let host = host();
    upsert_ts(&host, "/lib.ts", "export type Lib = { lib: string };\n");
    upsert_ts(
        &host,
        "/consumer.ts",
        "import type { Lib } from \"./lib\";\nexport type C = Lib;\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let scope = file_scope(&dispatch, "/consumer.ts");

    // `BareRef("Lib")` in /consumer.ts — `Lib` is an import binding (the eager
    // prepared-decl `name_resolution` fast-path would carry it). The carrier
    // path has NO name_resolution map and must rehydrate it through
    // `resolve_bare_name_in_scope` over the scope payload + shallow facts.
    let carrier = bare_ref_carrier(&dispatch, "Lib", scope, &[]);
    let via_carrier = resolve_subject(&dispatch, carrier, ProjectionMode::Navigate);

    // The eager path WITH a populated name_resolution (built from the consumer's
    // prepared decl) — the fast-path target.
    let eager = eager_resolved_with_name_resolution(
        &dispatch,
        &TypeExpr::Ref {
            name: Arc::from("Lib"),
            type_arguments: verter_type_expr::empty_type_args(),
        },
        "/consumer.ts",
        ProjectionMode::Navigate,
    );
    assert_eq!(
        via_carrier, eager,
        "the carrier head must resolve the import binding `Lib` to the SAME target the eager \
         name_resolution fast-path resolves — proving name_resolution was rehydrated, not dropped"
    );
    assert!(
        !is_bare_ref(&dispatch, via_carrier),
        "the import-binding head must resolve, not stay the bare carrier"
    );
    assert!(
        !is_opaque(&dispatch, via_carrier),
        "`Lib` import binding resolves"
    );
}

/// Eager lowering WITH a populated `name_resolution` (the prepared-decl map of
/// `canonical`'s primary decl) — exercises the `Ref` arm's fast-path — then
/// projected through the SAME empty-path `ProjectPath` for stage-parity.
fn eager_resolved_with_name_resolution(
    dispatch: &ProjectSemanticDispatch<'_>,
    expr: &TypeExpr,
    canonical: &str,
    mode: ProjectionMode,
) -> SemanticNodeId {
    let scope = file_scope(dispatch, canonical);
    let env: FxHashMap<String, SemanticNodeId> = FxHashMap::default();
    let scope_payload = dispatch.ctx.prepared_decl_bundle(canonical).map(|bundle| {
        crate::resolver_core::bare_name_resolve::DeclarationScopePayload::from_bundle(&bundle)
    });
    // Build the populated name_resolution from the consumer's import binding so
    // the FAST-PATH fires (mirrors the prepared-decl map's import entry).
    let mut name_resolution: FxHashMap<String, ResolvedRootIdentity> = FxHashMap::default();
    if let Some(bundle) = dispatch.ctx.prepared_decl_bundle(canonical) {
        for (local, binding) in bundle.import_bindings.iter() {
            name_resolution.insert(
                local.clone(),
                ResolvedRootIdentity::new(&binding.canonical_id, &binding.exported_name),
            );
        }
    }
    let shadowing = ScopeShadowing::from_scope_payload(scope_payload.as_ref());
    let mut substitutions: Vec<(Arc<str>, SemanticNodeId)> = Vec::new();
    let lowered = dispatch.shallow_lower_type_expr_with_context(
        expr,
        &env,
        &scope,
        &name_resolution,
        scope_payload.as_ref(),
        &shadowing,
        &mut substitutions,
        ProjectionReductionContext::published(mode),
    );
    resolve_subject(dispatch, lowered, mode)
}

// ── B15. NORMAL PATH WALKER re-enters carrier normalization ─────────────────
//
// A `BareRef` / `ImportType` carrier reached MID-WALK by the normal PathWalker
// (behind an alias / instantiation body, not as the top-level subject) must
// re-enter the SAME shared `resolve_carrier_subject_node` normalization the
// entry + shallow-synth use, then continue the walk from the resolved node —
// NOT terminate as `Opaque(Miss)`.
//
// Construction: `Alias(BareRef("Foo"))` as the base of a `[a]` path. The walker
// unwraps the Alias, reaching the `BareRef("Foo")` carrier as `current` WITH a
// `.a` segment still pending. Pre-fix the carrier hits the terminal-miss arm
// (lumped with Primitive/Literal/…) and the walk returns `Opaque(Miss)` —
// `first_member` / the `.a` projection FAILS. Post-fix the arm re-enters
// normalization, resolves `Foo` to its decl, and the walk projects `.a` →
// `string`.
#[test]
fn normal_path_walker_reenters_carrier_normalization_behind_alias() {
    let host = host();
    upsert_ts(&host, "/aw.ts", "export type Foo = { a: string };\n");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = dispatch.graph();
    let scope = file_scope(&dispatch, "/aw.ts");

    // `Alias(BareRef("Foo"))` — the carrier is reached behind the alias, NOT as
    // the top-level subject (so the entry-time normalization does not pre-resolve
    // it; only the in-walk re-entry can).
    let carrier = bare_ref_carrier(&dispatch, "Foo", scope.clone(), &[]);
    let alias = graph.intern_node_with_scope(SemanticNodeData::Alias(carrier), scope);

    // Project `.a` over the alias-wrapped carrier in Navigate.
    let projected = match dispatch.execute_type_node(SemanticQueryKey::ProjectPath {
        base: alias,
        path: Arc::from(vec![PathSegment::Member(Arc::from("a"))].into_boxed_slice()),
        context: ProjectionReductionContext::published(ProjectionMode::Navigate),
    }) {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        QueryResult::Recursive(id) => id,
        other => {
            panic!("ProjectPath over the alias-wrapped carrier did not yield a node: {other:?}")
        }
    };

    assert!(
        !is_opaque(&dispatch, projected),
        "the normal path walker must re-enter carrier normalization for the alias-wrapped \
         `BareRef(Foo)` and project `.a` — NOT terminate as Opaque(Miss): {:?}",
        dispatch.graph().node_data(projected).as_deref()
    );
    // `.a`'s value is `string`.
    assert!(
        matches!(
            dispatch.graph().node_data(projected).as_deref(),
            Some(SemanticNodeData::Primitive(PrimitiveKind::String))
        ),
        "projecting `.a` through the resolved `Foo` must yield `string`; got {:?}",
        dispatch.graph().node_data(projected).as_deref()
    );
}

// ── B16. NORMAL PATH WALKER keeps Opaque(Miss) for a genuinely-unresolvable
//        carrier ─────────────────────────────────────────────────────────────
//
// The in-walk carrier re-entry must keep the terminal `Opaque(Miss)` FALLBACK
// for a carrier that does NOT resolve (an unknown name). A carrier behind an
// alias whose head names nothing must still miss — the re-entry resolves to
// itself (unchanged) and the walk yields the honest miss, not a hang/panic.
#[test]
fn normal_path_walker_unresolvable_carrier_behind_alias_misses() {
    let host = host();
    upsert_ts(&host, "/aw2.ts", "export type Real = { a: string };\n");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = dispatch.graph();
    let scope = file_scope(&dispatch, "/aw2.ts");

    // `Alias(BareRef("DoesNotExist"))` — the head names no symbol in /aw2.ts.
    let carrier = bare_ref_carrier(&dispatch, "DoesNotExist", scope.clone(), &[]);
    let alias = graph.intern_node_with_scope(SemanticNodeData::Alias(carrier), scope);

    let projected = match dispatch.execute_type_node(SemanticQueryKey::ProjectPath {
        base: alias,
        path: Arc::from(vec![PathSegment::Member(Arc::from("a"))].into_boxed_slice()),
        context: ProjectionReductionContext::published(ProjectionMode::Navigate),
    }) {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        QueryResult::Recursive(id) => id,
        other => panic!("expected a node for the unresolvable carrier walk: {other:?}"),
    };
    assert!(
        is_opaque(&dispatch, projected),
        "an unresolvable carrier behind an alias must keep the terminal Opaque(Miss) fallback; \
         got {:?}",
        dispatch.graph().node_data(projected).as_deref()
    );
}

// ── B11. `CarrierResolverContext` CONSUMED ──────────────────────────────────
//
// Build a `CarrierResolverContext` explicitly and drive the shared head
// resolver through it — proving the context is no longer dead code and the
// shared `resolve_bare_ref_head` reads its fields.
#[test]
fn carrier_resolver_context_drives_shared_head_resolver() {
    let host = host();
    upsert_ts(&host, "/ctx.ts", "export type Widget = { w: number };\n");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let scope = file_scope(&dispatch, "/ctx.ts");

    let env: FxHashMap<String, SemanticNodeId> = FxHashMap::default();
    let name_resolution: FxHashMap<String, ResolvedRootIdentity> = FxHashMap::default();
    let scope_payload = dispatch.ctx.prepared_decl_bundle("/ctx.ts").map(|bundle| {
        crate::resolver_core::bare_name_resolve::DeclarationScopePayload::from_bundle(&bundle)
    });
    let shadowing = ScopeShadowing::from_scope_payload(scope_payload.as_ref());
    let ctx = CarrierResolverContext::new(
        &env,
        &scope,
        &name_resolution,
        scope_payload.as_ref(),
        &shadowing,
        ProjectionReductionContext::published(ProjectionMode::Navigate),
    );

    let name: Arc<str> = Arc::from("Widget");
    let empty_args: Arc<[SemanticNodeId]> = Arc::from(Vec::new().into_boxed_slice());
    let resolved = dispatch.resolve_bare_ref_head(&ctx, &name, 0, || Arc::clone(&empty_args));

    assert!(
        !is_bare_ref(&dispatch, resolved),
        "the shared head resolver driven through CarrierResolverContext must resolve `Widget`"
    );
    assert!(!is_opaque(&dispatch, resolved), "`Widget` resolves");

    // Drive the SAME helper in Expanded mode (a second context) so it routes
    // through `Instantiate` and materializes `Widget`'s body `{ w: number }` —
    // the discriminating body check (a Navigate `DeclRef` is not unwrapped by
    // the empty-path terminal, by design).
    let ctx_expanded = CarrierResolverContext::new(
        &env,
        &scope,
        &name_resolution,
        scope_payload.as_ref(),
        &shadowing,
        ProjectionReductionContext::published(ProjectionMode::Expanded),
    );
    let expanded = dispatch.resolve_bare_ref_head(&ctx_expanded, &name, 0, || empty_args);
    let (member, kind) = first_member_primitive(&dispatch, expanded)
        .expect("the Expanded-mode head resolver must materialize `Widget`'s body `{ w: number }`");
    assert_eq!(member, "w");
    assert_eq!(kind, PrimitiveKind::Number);
}

// ── B12. NO `Ref`-path drift ────────────────────────────────────────────────
//
// The eager `TypeExpr::Ref` lowering — now routed through the shared
// `resolve_bare_ref_head` helper — produces the SAME resolved node it did
// before the refactor. A bare `Foo` lowered via the eager path resolves to
// Foo's body, unchanged.
#[test]
fn eager_ref_path_has_no_drift_after_shared_helper() {
    let host = host();
    upsert_ts(&host, "/drift.ts", "export type Foo = { a: string };\n");
    let dispatch = ProjectSemanticDispatch::new(&host);

    // Eager `Ref { Foo }` lowering through `shallow_lower_type_expr_with_context`
    // (the production lowering arm, now calling the shared helper) resolves to a
    // non-opaque, non-bare-carrier node that expands to Foo's body.
    let resolved = eager_lower_subject(
        &dispatch,
        &TypeExpr::Ref {
            name: Arc::from("Foo"),
            type_arguments: verter_type_expr::empty_type_args(),
        },
        "/drift.ts",
        ProjectionMode::Navigate,
    );
    assert!(
        !is_opaque(&dispatch, resolved),
        "the eager `Ref {{ Foo }}` path must still resolve after the refactor"
    );
    assert!(
        !is_bare_ref(&dispatch, resolved),
        "the eager `Ref` arm must NOT regress to leaving a bare carrier"
    );
    assert!(
        is_decl_ref(&dispatch, resolved),
        "the eager Navigate `Ref {{ Foo }}` must resolve to a `DeclRef` carrier (unchanged): {:?}",
        dispatch.graph().node_data(resolved).as_deref()
    );

    // The eager `Ref { Foo }` lowered in EXPANDED mode routes through
    // `ResolveDecl` + `Instantiate` to Foo's body `{ a: string }` — unchanged by
    // the refactor (the eager arm now calls the shared helper).
    let expanded = eager_lower_subject(
        &dispatch,
        &TypeExpr::Ref {
            name: Arc::from("Foo"),
            type_arguments: verter_type_expr::empty_type_args(),
        },
        "/drift.ts",
        ProjectionMode::Expanded,
    );
    let (member, kind) = first_member_primitive(&dispatch, expanded)
        .expect("eager `Foo` (Expanded) must materialize its body `{ a: string }`");
    assert_eq!(member, "a");
    assert_eq!(kind, PrimitiveKind::String);
}

// ── B13. IMPORTED BUILTIN NAME — the import wins over the ambient builtin ────
//
// `import type { Partial } from "./alias"` brings a USERLAND type NAMED `Partial`
// into scope. A `BareRef("Partial")` carrier in that file must resolve to the
// IMPORTED userland `Partial`, NOT the ambient `__builtin__.Partial` — matching
// the eager path, whose populated `name_resolution` carries the import binding
// and so suppresses the builtin fast-path.
//
// The carrier path rehydrates an EMPTY `name_resolution`, so suppression must
// come from the scope payload's import bindings flowing into `ScopeShadowing`.
// Pre-fix `ScopeShadowing::from_scope_payload` omits `import_bindings`, the
// builtin fast-path fires, and `Partial` resolves to the ambient builtin's
// mapped-type surface (no bare `mine` member) — every assertion below FAILS.
// Post-fix the import binding shadows the builtin and the userland body wins.
#[test]
fn carrier_head_imported_builtin_name_resolves_import_not_builtin() {
    let host = host();
    // A userland type whose name collides with the ambient builtin `Partial`.
    upsert_ts(
        &host,
        "/alias.ts",
        "export type Partial<T> = { mine: T };\n",
    );
    upsert_ts(
        &host,
        "/consumer.ts",
        "import type { Partial } from \"./alias\";\nexport type C = Partial<number>;\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let scope = file_scope(&dispatch, "/consumer.ts");

    // `BareRef("Partial", [number])` in /consumer.ts. `Partial` is an import
    // binding (collides with the builtin). The carrier path must resolve the
    // IMPORT, not the builtin.
    let carrier = bare_ref_carrier(&dispatch, "Partial", scope, &[PrimitiveKind::Number]);
    let resolved = resolve_subject(&dispatch, carrier, ProjectionMode::Navigate);

    assert!(
        !is_bare_ref(&dispatch, resolved),
        "the imported `Partial` head must resolve, not stay the bare carrier"
    );
    assert!(
        !is_opaque(&dispatch, resolved),
        "imported `Partial<number>` resolves"
    );

    // Equivalence: the carrier path must resolve to the SAME node the eager path
    // resolves with a populated `name_resolution` (the import-binding fast-path).
    let eager = eager_resolved_with_name_resolution(
        &dispatch,
        &TypeExpr::Ref {
            name: Arc::from("Partial"),
            type_arguments: Arc::from(
                vec![TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)]
                    .into_boxed_slice(),
            ),
        },
        "/consumer.ts",
        ProjectionMode::Navigate,
    );
    assert_eq!(
        resolved, eager,
        "the carrier head must resolve the IMPORTED `Partial` to the SAME target the eager \
         name_resolution fast-path resolves — the import binding suppresses the builtin \
         fast-path, so an `import type {{ Partial }}` wins over `__builtin__.Partial`"
    );

    // Discriminating body check: the IMPORTED userland `Partial<T> = { mine: T }`
    // materializes a bare `mine` member; the ambient `__builtin__.Partial`'s
    // mapped-type surface over `number` does NOT surface a bare `mine`.
    let (member, kind) = first_member_primitive(&dispatch, carrier)
        .expect("imported userland `Partial<number>` must expand to `{ mine: number }`");
    assert_eq!(
        member, "mine",
        "the IMPORTED userland `Partial` (member `mine`) must win over the builtin `Partial`"
    );
    assert_eq!(kind, PrimitiveKind::Number);
}

// ── B14. `local_scope` PRESERVED in the rehydrated scope ─────────────────────
//
// `resolve_carrier_subject_node` must rehydrate the resolver context from the
// carrier's ACTUAL scope (including its `local_scope`), not a `local_scope`-
// dropped `canonical_file()` rebuild. A `BareRef` carrier whose head scope
// carries `local_scope: Some(..)` must resolve against that exact scope.
//
// Discrimination: the carrier is constructed with a non-None `local_scope`; the
// resolved node's origin scope must carry the SAME `local_scope`. A
// `canonical_file()`-rebuilt scope (the divergence the brief named) would drop
// `local_scope` to `None`, failing the equality below.
#[test]
fn carrier_head_preserves_local_scope_in_rehydrated_scope() {
    let host = host();
    upsert_ts(&host, "/ls.ts", "export type Foo = { a: string };\n");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let shallow = dispatch
        .ctx
        .shallow_file_state("/ls.ts")
        .expect("/ls.ts indexed");

    // A carrier scope carrying a NON-None `local_scope`.
    let local: u32 = 0xABCD;
    let scope_with_local = NodeScopeId::File {
        canonical_id: Arc::from("/ls.ts"),
        whole_hash: shallow.whole_hash,
        local_scope: Some(local),
    };
    let carrier = bare_ref_carrier(&dispatch, "Foo", scope_with_local, &[]);
    let resolved = resolve_subject(&dispatch, carrier, ProjectionMode::Navigate);

    assert!(!is_bare_ref(&dispatch, resolved), "`Foo` head must resolve");
    assert!(!is_opaque(&dispatch, resolved), "`Foo` resolves");

    // The resolved node's origin scope must preserve the carrier's `local_scope`
    // (the head resolver interned the resolved node under the carrier's scope).
    let resolved_scope = dispatch
        .graph()
        .node_scope(resolved)
        .expect("resolved node must carry an origin scope");
    match resolved_scope {
        NodeScopeId::File { local_scope, .. } => assert_eq!(
            local_scope,
            Some(local),
            "the rehydrated scope must PRESERVE the carrier's `local_scope` — a \
             `canonical_file()` rebuild would drop it to None"
        ),
        NodeScopeId::Global => panic!("resolved `Foo` must carry a File scope, not Global"),
    }
}

// ── B17. EAGER FOLD: an unresolvable head does NOT lower+dispatch dead args ──
//
// The eager `TypeExpr::Ref` arm must resolve/classify the head BEFORE lowering
// its type-args, so an UNRESOLVABLE head under an EAGER mode does not
// lower+dispatch its args (which loads files, marks partials, hits fuses for
// dead syntax). The fold regressed this by lowering all args UNCONDITIONALLY at
// the top before calling the head helper.
//
// The eager modes (`Expanded` / `Identity`) ARE the resolving demand: a head
// that misses there is a conclusive `Opaque(Miss)` and its args are genuinely
// DEAD. Under the carrier modes (`Navigate` / `Shallow` / `Skeleton`) an
// unresolved head instead PRESERVES the scoped `BareRef` carrier — whose args
// are LIVE carrier content the demand points retry — see the Navigate
// companion test below.
//
// Discriminator: `Ref { name: "Unknown", type_arguments: [Ref { ArgT }] }` in
// /main.ts, where `ArgT` is IMPORTED from /argfile.ts. Lowering `ArgT` resolves
// the import binding and INDEXES /argfile.ts (an observable file-load via
// `FileArtifactStore::get_any`). Upsert does NOT pre-index (verified), so
// /argfile.ts is indexed IFF the dead arg was lowered. Pre-fix the arg lowers
// unconditionally → /argfile.ts IS indexed → assertion FAILS. Post-fix the
// `Unknown` head misses first → `ArgT` never lowers → /argfile.ts NOT indexed.
#[test]
fn eager_unresolvable_ref_head_does_not_lower_dead_type_args() {
    let host = host();
    upsert_ts(&host, "/argfile.ts", "export type ArgT = { x: number };\n");
    upsert_ts(
        &host,
        "/main.ts",
        "import type { ArgT } from \"./argfile\";\nexport type Unused = ArgT;\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);

    // Sanity: /argfile.ts is NOT indexed before the query (upsert does not
    // eagerly index a dependency).
    assert!(
        dispatch
            .ctx
            .host_for_fact_tracer_install()
            .project_type_store()
            .indexed()
            .get_any("/argfile.ts")
            .is_none(),
        "precondition: /argfile.ts must not be indexed before the query"
    );

    // Lower `Unknown<ArgT>` through the eager arm in /main.ts under the EAGER
    // `Expanded` mode. `Unknown` resolves to nothing; `ArgT` (the dead arg)
    // imports from /argfile.ts.
    let expr = TypeExpr::Ref {
        name: Arc::from("Unknown"),
        type_arguments: Arc::from(
            vec![TypeExpr::Ref {
                name: Arc::from("ArgT"),
                type_arguments: verter_type_expr::empty_type_args(),
            }]
            .into_boxed_slice(),
        ),
    };
    let resolved = eager_lower_subject(&dispatch, &expr, "/main.ts", ProjectionMode::Expanded);

    // The `Unknown` head is unresolvable — the eager demand's honest miss.
    assert!(
        is_opaque(&dispatch, resolved),
        "an unresolvable `Unknown<ArgT>` head must miss under Expanded; got {:?}",
        dispatch.graph().node_data(resolved).as_deref()
    );
    // DISCRIMINATING: the dead `ArgT` arg must NOT have been lowered, so
    // /argfile.ts must NOT have been indexed by this query.
    assert!(
        dispatch
            .ctx
            .host_for_fact_tracer_install()
            .project_type_store()
            .indexed()
            .get_any("/argfile.ts")
            .is_none(),
        "the eager arm must NOT lower+dispatch the dead arg `ArgT` of an unresolvable head — \
         /argfile.ts was indexed, proving the dead arg was lowered (the pre-fix \
         unconditional-lower regression)"
    );
}

// ── B21. NAVIGATE TRANSIT: an unresolvable head PRESERVES the BareRef carrier
//         (name + args + scope), never `Opaque(Miss)` ────────────────────────
//
// The carrier-mode counterpart of B17: under `Navigate` transit an unresolved
// authored reference must remain a scoped `BareRef` carrier — the authored
// head's name, its (lowered, LIVE) type-argument nodes, and its scope are
// semantic content the demand points retry (`Navigate` retries identity,
// `Expanded` resolves + executes). Collapsing to `Opaque(Miss)` at lowering
// destroys them (the raise-time info loss this contract forbids).
#[test]
fn navigate_unresolvable_ref_head_preserves_bare_ref_carrier_with_args() {
    let host = host();
    upsert_ts(&host, "/argfile.ts", "export type ArgT = { x: number };\n");
    upsert_ts(
        &host,
        "/main.ts",
        "import type { ArgT } from \"./argfile\";\nexport type Unused = ArgT;\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);

    let expr = TypeExpr::Ref {
        name: Arc::from("Unknown"),
        type_arguments: Arc::from(
            vec![TypeExpr::Ref {
                name: Arc::from("ArgT"),
                type_arguments: verter_type_expr::empty_type_args(),
            }]
            .into_boxed_slice(),
        ),
    };
    let resolved = eager_lower_subject(&dispatch, &expr, "/main.ts", ProjectionMode::Navigate);

    // The head stays a scoped BareRef carrier named `Unknown`, with the ONE
    // applied argument lowered and preserved on the carrier.
    let data = dispatch
        .graph()
        .node_data(resolved)
        .expect("carrier node data");
    let (name, scope) = data
        .bare_ref_head()
        .expect("Navigate transit must preserve the unresolved head as a BareRef carrier");
    assert_eq!(name.as_ref(), "Unknown", "the authored head name survives");
    match scope {
        NodeScopeId::File { canonical_id, .. } => assert_eq!(
            canonical_id.as_ref(),
            "/main.ts",
            "the carrier is scoped to the lowering file"
        ),
        NodeScopeId::Global => panic!("the carrier must carry the file scope, not Global"),
    }
    let args = data.carrier_type_args();
    assert_eq!(args.len(), 1, "the applied argument list survives");
    // The LIVE arg resolved through the shared head resolver: `ArgT` is a
    // resolvable import, so the carrier's argument is its `DeclRef`
    // identity carrier — proving the args were lowered FOR the carrier
    // (they are retry content, not dead syntax).
    assert!(
        matches!(
            dispatch.graph().node_data(args[0]).as_deref(),
            Some(SemanticNodeData::DeclRef { identity })
                if identity.canonical_id.as_ref() == "/argfile.ts"
                    && identity.decl_name.as_ref() == "ArgT"
        ),
        "the preserved carrier's argument must be the resolved `DeclRef(ArgT)`; got {:?}",
        dispatch.graph().node_data(args[0]).as_deref()
    );
}

// ── B18. EAGER FOLD: a RESOLVABLE head still lowers+applies its args (no drift)
//
// The lazy-arg ordering must NOT regress the resolvable path: a `Ref` head that
// DOES resolve must still lower its args and apply them. `Box<ArgT>` over
// `type Box<T> = { v: T }` (with `ArgT` imported) must instantiate to
// `{ v: { x: number } }` (Expanded) — proving the args were lowered on the
// resolved branch AND /argfile.ts WAS indexed (the arg is live here).
#[test]
fn eager_resolvable_ref_head_still_lowers_and_applies_args() {
    let host = host();
    upsert_ts(&host, "/argfile2.ts", "export type ArgT = { x: number };\n");
    upsert_ts(
        &host,
        "/boxmain.ts",
        "import type { ArgT } from \"./argfile2\";\nexport type Box<T> = { v: T };\nexport type Unused = Box<ArgT>;\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);

    let expr = TypeExpr::Ref {
        name: Arc::from("Box"),
        type_arguments: Arc::from(
            vec![TypeExpr::Ref {
                name: Arc::from("ArgT"),
                type_arguments: verter_type_expr::empty_type_args(),
            }]
            .into_boxed_slice(),
        ),
    };
    // Expanded so the resolved `Box<ArgT>` materializes its body.
    let resolved = eager_lower_subject(&dispatch, &expr, "/boxmain.ts", ProjectionMode::Expanded);
    assert!(
        !is_opaque(&dispatch, resolved),
        "the resolvable `Box<ArgT>` head must resolve+instantiate, not miss"
    );
    // The live arg `ArgT` MUST have been lowered → /argfile2.ts indexed.
    assert!(
        dispatch
            .ctx
            .host_for_fact_tracer_install()
            .project_type_store()
            .indexed()
            .get_any("/argfile2.ts")
            .is_some(),
        "a RESOLVABLE head must still lower its live args — /argfile2.ts must be indexed"
    );
    // Body `{ v: <ArgT> }` — member `v` whose value is the SUBSTITUTED `ArgT`
    // (a reference into /argfile2.ts), NOT a surviving free `TypeParam(T)` (which
    // a dropped-args resolver would leave).
    let data = dispatch
        .graph()
        .node_data(resolved)
        .expect("resolved node data");
    let SemanticNodeData::Object(view) = data.as_ref() else {
        panic!(
            "`Box<ArgT>` (Expanded) must materialize an Object; got {:?}",
            data.as_ref()
        );
    };
    let member = view
        .members
        .iter()
        .find(|m| m.name.as_ref() == "v")
        .expect("`Box`'s `v` member must be present");
    let value_data = dispatch.graph().node_data(member.value);
    // Discriminating: `v`'s value must be the substituted `ArgT` (an Object body
    // OR a decl reference/placeholder into /argfile2.ts), proving the live arg
    // was lowered + applied. A dropped-args resolver leaves a free `TypeParam`.
    assert!(
        !matches!(
            value_data.as_deref(),
            Some(SemanticNodeData::TypeParam { .. })
        ),
        "`v`'s value must NOT be a surviving free `TypeParam(T)` — the live arg `ArgT` must be \
         lowered + substituted; got {:?}",
        value_data.as_deref()
    );
    let value_dbg = format!("{:?}", value_data.as_deref());
    assert!(
        value_dbg.contains("argfile2")
            || matches!(value_data.as_deref(), Some(SemanticNodeData::Object(_))),
        "`v`'s value must reference the substituted `ArgT` (from /argfile2.ts); got {value_dbg}"
    );
}
