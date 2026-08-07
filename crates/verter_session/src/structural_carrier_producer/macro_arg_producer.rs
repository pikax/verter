//! The macro-argument producer — the SINGLE module that owns everything
//! producer-capable: the query-free structural lowerer, the macro hot
//! mirror, and the `<script setup generic="…">` binder-seed builder.
//!
//! ## Single-producer boundary (two confinement regimes)
//!
//! Verter has exactly ONE structural-carrier producer. The producer-capable
//! builders — the raw lowerer [`lower_type_expr_structural`], the macro
//! hot-mirror builder [`build_macro_hot_ref`], and the scope builder
//! [`build_script_setup_seed_frames`] — are PRIVATE to THIS module (no
//! visibility modifier). The ONLY crate-visible items this module exposes are
//! [`macro_type_arg_hot_ref`] (the sole production entry that lowers a macro
//! `parsed_type_argument`) and the [`MacroHotMirror`] artifact child it
//! populates.
//!
//! The single-producer guarantee has TWO regimes, NOT one. (1) The FOREIGN
//! case is COMPILER-CONFINED: a module OUTSIDE this one cannot NAME the
//! private builders — a foreign reference is a compile error (E0603 / E0433),
//! not a lint, so a second producer in a foreign module is unrepresentable by
//! construction. (2) The SAME-MODULE case is NOT compiler-confined: Rust
//! privacy is module-scoped, so a SECOND producer written INSIDE this file CAN
//! name the module-private builders; keeping producer-capable code in one file
//! does not make that a compile error. That same-module residual is
//! POLICED by the strengthened single-producer architecture guards
//! (cfg-satisfiability classification + crate-visible producer-exposure
//! collector covering fn / value / trait entries + the no-codegen-surface
//! scan + the no-query/dispatch purity scan + the scoped derive-shadow
//! import check). The IRREDUCIBLE residual is trust in this one sanctioned
//! producer implementation plus compiler bugs / build-time substitution /
//! out-of-tree proc-macros — NOT covered by either regime, by design.
//!
//! ## The query-free structural lowering
//!
//! [`lower_type_expr_structural`] EMITS the unresolved carriers (`BareRef`,
//! `ImportType`, `RawFallback`, `SyntheticBinding`, kind-preserving
//! `Signature` nodes, and
//! the tuple-element `rest` flag) alongside the structural shells (objects,
//! functions, unions, intersections, tuples, operators) from the
//! [`TypeExpr`] the OXC worker produces. It performs NO name / import / type
//! resolution or reduction:
//!
//! - a bare `Foo` becomes a `BareRef` carrier, never a resolved `DeclRef`
//!   or `InstantiationRef`;
//! - an `import("…")` becomes an `ImportType` carrier, never a module
//!   lookup;
//! - a `keyof` / indexed-access / conditional / mapped / `typeof` operator
//!   becomes its deferred structural shell carrying structurally-lowered
//!   operands, never a reduced result — even where the eager path reduces
//!   it.
//!
//! Resolution and reduction are a later, demand-time concern that consumes
//! these carriers; this lowerer never anticipates them. The lowerer is
//! graph-local: it reaches the [`SemanticGraphStore`] to intern the
//! structural nodes it builds and read them back (e.g. the distributive
//! check / mapper classification); the owner supplies the rooting
//! [`NodeScopeId`]. It never touches `ProjectSemanticDispatch`, a resolver
//! context, a `SemanticQueryKey`, or any host / type-provider state — the
//! `session_graph_lowerer_makes_no_query` guard locks that statically.
//!
//! ## The macro hot mirror
//!
//! The hot mirror is the SINGLE-ENTRY producer of a macro's type argument
//! ([`AnalyzedMacro.parsed_type_argument`](verter_semantic::analysis::AnalyzedMacro))
//! graph node. The eager, CONTEXT-SHAPED (per the caller's
//! [`ProjectionMode`](crate::semantic_query::ProjectionMode)) per-site lowering
//! it replaced produced a one-demand-only reduction — not a storable, shared,
//! mode-neutral handle. The four production macro-arg sites
//! (`meta_resolve::slot_binding_graph`, `meta_resolve::projectors`,
//! `host_manage::eval_env`, and `typeinfo::framework_surface::vue_exec`) now
//! READ this mirror handle instead of lowering the macro arg themselves.
//!
//! On first demand per `macro_index` it lowers the macro's
//! `parsed_type_argument` ONCE through [`lower_type_expr_structural`] into a
//! [`HotTypeRef`] — an interned [`SemanticNodeId`] carrying the unresolved
//! `BareRef` / `ImportType` / operator-shell carriers, NO resolution. Every
//! production site that needs a macro type-argument graph node reads THIS
//! handle and re-enters the ONE shared dispatch (`SemanticQueryKey` →
//! `ProjectSemanticDispatch::execute`) at its own demand / mode (Navigate, a
//! `ProjectPath` for an indexed-access or per-field path, a Shallow surface).
//! Different TERMINAL demands are fine; a second BASE producer of the macro
//! arg's graph node is not — that is the forbidden callsite-scattered
//! structural-vs-eager dual path.
//!
//! ## Laziness / content addressing / singleflight
//!
//! [`MacroHotMirror`] is a FILE-ARTIFACT child stored adjacent to the
//! macros + lazy [`DeclBodyMemo`](crate::decl_body_memo::DeclBodyMemo) on
//! [`IndexedReady`](crate::project_type_store::IndexedReady) — it mirrors the
//! memo shape. Its identity is the owning artifact's `(canonical,
//! whole_hash)` plus the `macro_index`; a content edit publishes a fresh
//! `IndexedReady` carrying a fresh empty mirror, so a superseded mirror can
//! never answer a new-content demand. Publishing an artifact lowers ZERO
//! macro mirrors (the cell table is unallocated until first demand). The
//! passive mirror storage lazily allocates one slot per macro on first demand.
//! Each slot singleflights cold population and provides a lock-free committed
//! read. A transient `LeaseMiss` leaves its slot vacant for a later retry.
//!
//! ## Script-setup generic seeding
//!
//! A `<script setup generic="T">` parameter must lower to its
//! [`SemanticNodeData::TypeParam`] binder, NOT a `BareRef(T)`. The
//! structural lowerer does not consult the host's script-setup bindings; it
//! only does a syntactic in-scope binder lookup. So the builder pre-builds a
//! SEED [`BinderScope`] frame by re-sourcing the `<script setup generic="…">`
//! clause from the owner's ROUTE-FREE local [`IndexedReady`] data
//! (`raw_source` + `framework_parse`, through `sfc_script_setup_type_params`)
//! — interning a `TypeParam` node matching the eager path's `<script-setup>`
//! decl sentinel + ordinal + lowered constraint / default shape — and passes
//! it to the lowerer, so `lookup_binder("T")` returns the binder node. This
//! is owner-local shallow scope data (NOT the prepared-decl bundle, whose
//! cold path can route-resolve imports), keeping the producer PURE. The seed
//! frame is built incrementally so an earlier binder is visible to a later
//! one's constraint / default (TS scoping).

use std::cell::Cell;
use std::sync::{Arc, OnceLock};

use rustc_hash::FxHashMap;
use verter_type_expr::{FunctionExpr, LiteralValue, MappedModifier, ObjectMember, TypeExpr};

use super::infer_binder_names::{
    collect_extends_infer_declarations, BinderScope, InferSyntaxPathStep, StructuralLowerContext,
};
use crate::resolver_core::ResolverContext;
use crate::semantic_query::{
    AuthoredPropertyKey, DeclIdentity, FunctionParam, HotTypeRef, IndexKey, IndexSignature,
    MacroOwnBodyStamp, MapperKey, MapperKind, NodeScopeId, OptionalityMod, PrimitiveKind,
    QueryError, ReadonlyMod, ScopeId, SemanticNodeData, SemanticNodeId, SignatureKind,
    SurfaceMember, SyntheticBindingId, TupleElement, TypeParamDecl, ValueRootKey,
};
use crate::semantic_query_memo::SemanticGraphStore;

// =============================================================================
// Query-free structural lowering internals (PRIVATE to this module).
// =============================================================================

/// A `TypeExpr` shape the structural lowerer genuinely cannot construct
/// without resolution.
///
/// This is a real typed error, NEVER an `Unknown`-as-control-flow signal:
/// the lowerer always prefers emitting a typed carrier (`RawFallback`,
/// `BareRef`, …) over erroring, so this variant is reserved for inputs that
/// have no faithful unresolved representation at all (e.g. a solver-minted
/// `RecursiveRef`, which is never produced by fresh OXC lowering and cannot
/// be reconstructed structurally).
#[derive(Debug, Clone, PartialEq, Eq)]
enum StructuralLowerError {
    /// `shape` names the offending `TypeExpr` variant for diagnostics.
    UnsupportedWithoutResolution { shape: &'static str },
}

/// Structurally lower an owned [`TypeExpr`] into the dormant semantic-graph
/// carriers, rooted at the owner-supplied `scope`, performing no resolution.
///
/// Returns a [`HotTypeRef`] wrapping the interned root node, or a typed
/// [`StructuralLowerError`] for a shape with no unresolved representation.
///
/// PRIVATE (no visibility modifier): reachable only from this module's own
/// producer paths ([`build_macro_hot_ref`], [`build_script_setup_seed_frames`])
/// and this module's in-module unit tests. TWO REGIMES: a FOREIGN second producer
/// is COMPILER-CONFINED — no other module can NAME this fn (a compile error,
/// E0603 / E0433), so it is unrepresentable by construction; a SAME-MODULE second
/// producer is NOT compiler-confined (Rust privacy is module-scoped, so code
/// written INSIDE this module CAN name it) and is instead POLICED by the bounded
/// single-producer architecture guards.
fn lower_type_expr_structural(
    graph: &SemanticGraphStore,
    expr: &TypeExpr,
    scope: NodeScopeId,
    ctx: &StructuralLowerContext<'_>,
) -> Result<HotTypeRef, StructuralLowerError> {
    // The mapped-binder ordinal counter is per-lowering, owned here so a
    // caller never has to supply one.
    let mapper_ordinals = Cell::new(0u16);
    let infer_binders = match ctx.infer_source {
        Some(source) => {
            crate::semantic_query::InferBinderFactory::for_authored_locator(&scope, expr, source)
        }
        None => crate::semantic_query::InferBinderFactory::new(&scope, expr),
    };
    let ctx = ctx
        .with_mapper_ordinals(&mapper_ordinals)
        .with_infer_binders(&infer_binders);
    let node = lower_node(graph, expr, &scope, &ctx)?;
    Ok(HotTypeRef::new(node))
}

/// Lower one `expr` to an interned [`SemanticNodeId`] under `scope`.
///
/// Emission arms are implemented one `TypeExpr` variant at a time under
/// test; see `structural_lower_tests`.
fn lower_node(
    graph: &SemanticGraphStore,
    expr: &TypeExpr,
    scope: &NodeScopeId,
    ctx: &StructuralLowerContext<'_>,
) -> Result<SemanticNodeId, StructuralLowerError> {
    match expr {
        // -- Structural terminals (no resolution) --
        TypeExpr::Primitive(name) => Ok(graph.intern_node_with_scope(
            SemanticNodeData::Primitive(crate::project_semantic_dispatch::map_primitive_name(
                *name,
            )),
            scope.clone(),
        )),
        TypeExpr::Literal(value) => {
            Ok(graph
                .intern_node_with_scope(SemanticNodeData::Literal(value.clone()), scope.clone()))
        }

        // -- Composite structural shells --
        // An empty union/intersection degenerates to `never`; a single arm
        // unwraps to that arm (matching the eager structural shape).
        TypeExpr::Union(arms) => lower_union_or_intersection(graph, arms, scope, ctx, true),
        TypeExpr::Intersection(arms) => lower_union_or_intersection(graph, arms, scope, ctx, false),
        TypeExpr::Array { element, readonly } => {
            let element = lower_node(graph, element, scope, ctx)?;
            Ok(graph.intern_node_with_scope(
                SemanticNodeData::Array {
                    element,
                    readonly: *readonly,
                },
                scope.clone(),
            ))
        }
        // The plain surface tuple shell, preserving per-element label /
        // optional / rest metadata. No variadic-spread normalization (that
        // is a reduction): open rest elements survive verbatim.
        TypeExpr::Tuple { elements, readonly } => {
            let mut lowered = Vec::with_capacity(elements.len());
            for el in elements.iter() {
                lowered.push(TupleElement {
                    label: el.label.as_deref().map(Arc::<str>::from),
                    value: lower_node(graph, &el.ty, scope, ctx)?,
                    optional: el.optional,
                    rest: el.rest,
                });
            }
            Ok(graph.intern_node_with_scope(
                SemanticNodeData::Tuple {
                    elements: Arc::from(lowered.into_boxed_slice()),
                    readonly: *readonly,
                },
                scope.clone(),
            ))
        }
        TypeExpr::TemplateLiteral {
            quasis,
            expressions,
        } => {
            let quasis: Arc<[Arc<str>]> = quasis.iter().map(|q| Arc::from(q.as_str())).collect();
            let expressions = lower_args(graph, expressions, scope, ctx)?;
            Ok(graph.intern_node_with_scope(
                SemanticNodeData::TemplateLiteral {
                    quasis,
                    expressions,
                },
                scope.clone(),
            ))
        }
        // Parenthesized types are structurally transparent: `(A | B)` lowers
        // exactly as `A | B`.
        TypeExpr::Parenthesized(inner) => lower_node(graph, inner, scope, ctx),

        // -- Deferred operator shells (NEVER reduced) --
        // The operand is structurally lowered and the operator survives as a
        // shell node carrying it; the structural lowerer never executes the
        // reduction, even where the eager path would.
        TypeExpr::KeyOf(operand) => {
            let base = lower_node(graph, operand, scope, ctx)?;
            Ok(graph.intern_node_with_scope(SemanticNodeData::KeyOf { base }, scope.clone()))
        }
        // The index operand classifies into a literal-string / canonical
        // literal-number key or a structurally-lowered type node; the access
        // always survives as a deferred shell (never executed/projected).
        TypeExpr::IndexedAccess { object, index } => {
            let object = lower_node(graph, object, scope, ctx)?;
            let index = match index.as_ref() {
                TypeExpr::Literal(LiteralValue::String(s)) => {
                    IndexKey::String(Arc::from(s.as_str()))
                }
                TypeExpr::Literal(LiteralValue::Number(n)) => {
                    match crate::semantic_query::index_key::integer_convention_index_key(*n) {
                        Some(i) => IndexKey::Number(i),
                        None => IndexKey::Computed(lower_node(graph, index, scope, ctx)?),
                    }
                }
                _ => IndexKey::Computed(lower_node(graph, index, scope, ctx)?),
            };
            Ok(graph.intern_node_with_scope(
                SemanticNodeData::IndexedAccess { object, index },
                scope.clone(),
            ))
        }
        // The deferred conditional shell carries all four structurally-lowered
        // operands; the branch is NEVER decided here. `distributive` is the
        // syntactic naked-type-parameter property of the check.
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            let infer_binders = ctx
                .infer_binders
                .expect("structural lowering injects an infer identity authority");
            let check = lower_node(graph, check, scope, ctx)?;
            // `infer P` names introduced by the `extends` clause bind for the
            // TRUE branch only (TS scoping). Collect them syntactically BEFORE
            // the lowered-extends id shadows the `extends` name, then intern the
            // matching `Infer` carrier under `scope` — the store is
            // content-addressed by `(data, scope)`, so this is the SAME node the
            // `extends` arm interns, and a true-branch `P` resolves to that
            // binder instead of leaking out as an unbound `BareRef`. The false
            // branch and the check / extends are unaffected. Purely syntactic
            // binder collection — no resolution.
            let extends_path = infer_binders
                .path_for_expr(expr)
                .child(InferSyntaxPathStep::ConditionalExtends);
            let infer_sites = collect_extends_infer_declarations(extends, &extends_path);
            let mut declaration_frame = BinderScope::default();
            let mut declarations = FxHashMap::default();
            for site in &infer_sites {
                let binder = infer_binders.binder_at(&site.path);
                let declaration = graph.intern_node_with_scope(
                    SemanticNodeData::Infer {
                        name: Arc::clone(&site.name),
                        binder: binder.clone(),
                    },
                    scope.clone(),
                );
                declaration_frame.bind_infer_declaration(Arc::clone(&site.name), declaration);
                declarations.insert(Arc::clone(&site.name), (declaration, binder));
            }
            let mut extends_frames: Vec<BinderScope> = ctx.binders.to_vec();
            extends_frames.push(declaration_frame);
            let extends_ctx = ctx.with_binders(&extends_frames);
            let extends = lower_node(graph, extends, scope, &extends_ctx)?;
            let true_branch_ref = if infer_sites.is_empty() {
                lower_node(graph, true_type, scope, ctx)?
            } else {
                let mut infer_frame = BinderScope::default();
                for site in &infer_sites {
                    // References bind to `InferRef`, never the `Infer`
                    // declaration node (the shadow stop keys on declarations).
                    let infer_node = graph.intern_node_with_scope(
                        SemanticNodeData::InferRef {
                            name: Arc::clone(&site.name),
                            binder: declarations
                                .get(&site.name)
                                .expect("conditional infer declaration was preseeded")
                                .1
                                .clone(),
                        },
                        scope.clone(),
                    );
                    infer_frame.bind(Arc::clone(&site.name), infer_node);
                }
                let mut frames: Vec<BinderScope> = ctx.binders.to_vec();
                frames.push(infer_frame);
                let true_ctx = ctx.with_binders(&frames);
                lower_node(graph, true_type, scope, &true_ctx)?
            };
            let false_branch_ref = lower_node(graph, false_type, scope, ctx)?;
            // This `match` is intentionally the de-sugared form of `matches!`:
            // a `matches!` body macro is forbidden in this producer module, so
            // the distributive check stays an explicit `match`. Clippy's
            // `match_like_matches_macro` suggestion to collapse this back to
            // `matches!` MUST NOT be applied here.
            #[allow(clippy::match_like_matches_macro)]
            let distributive = match graph.node_data(check).as_deref() {
                Some(SemanticNodeData::TypeParam { .. }) => true,
                _ => false,
            };
            Ok(graph.intern_node_with_scope(
                SemanticNodeData::Conditional {
                    check,
                    extends,
                    true_branch_ref,
                    false_branch_ref,
                    distributive,
                },
                scope.clone(),
            ))
        }

        // -- Degenerate / unconstructable shapes --
        // A standalone rest / `readonly` outside tuple context is
        // structurally transparent: there is NO standalone `Rest` graph
        // carrier (tuple-rest fidelity rides on `TupleElement.rest`), so the
        // inner operand lowers directly.
        TypeExpr::Rest(inner) => lower_node(graph, inner, scope, ctx),
        // A solver-minted recursive back-edge is never produced by fresh OXC
        // lowering and cannot be reconstructed without the resolution context
        // that minted it.
        TypeExpr::RecursiveRef { .. } => Err(StructuralLowerError::UnsupportedWithoutResolution {
            shape: "RecursiveRef",
        }),
        TypeExpr::Infer { name } => {
            if let Some(declaration) = ctx.lookup_infer_declaration(name) {
                let declaration_data = graph.node_data(declaration);
                if let Some(SemanticNodeData::Infer { .. }) = declaration_data.as_deref() {
                    return Ok(declaration);
                }
            }
            Ok(graph.intern_node_with_scope(
                SemanticNodeData::Infer {
                    name: Arc::from(name.as_str()),
                    binder: ctx
                        .infer_binders
                        .expect("structural lowering injects an infer identity authority")
                        .binder_for_expr(expr),
                },
                scope.clone(),
            ))
        }

        // -- Call / construct signatures — ONE `Signature` carrier whose
        //    `kind` preserves the spelling (`new () => R` stays distinct
        //    from `() => R`).
        TypeExpr::Function(func) => {
            lower_function_signature(graph, func, scope, ctx, SignatureKind::Call)
        }
        TypeExpr::ConstructorType(func) => {
            lower_function_signature(graph, func, scope, ctx, SignatureKind::Construct)
        }

        // -- First-class type-parameter reference --
        // A bound name returns its binder node; an unbound one interns a
        // `TypeParam` node under a file-scoped name identity (matching the
        // eager unbound-parameter shape), never a host resolution.
        TypeExpr::TypeParameter(param) => {
            if let Some(binder) = ctx.lookup_binder(&param.name) {
                return Ok(binder);
            }
            let constraint = param
                .constraint
                .as_deref()
                .map(|c| lower_node(graph, c, scope, ctx))
                .transpose()?;
            let default = param
                .default
                .as_deref()
                .map(|d| lower_node(graph, d, scope, ctx))
                .transpose()?;
            let display_name: Arc<str> = Arc::from(param.name.as_str());
            let decl = DeclIdentity::from_scope(scope, Arc::clone(&display_name));
            Ok(graph.intern_node_with_scope(
                SemanticNodeData::TypeParam {
                    decl,
                    param_index: 0,
                    constraint,
                    default,
                    display_name,
                },
                scope.clone(),
            ))
        }

        // -- Named reference --
        // A bare name that hits a syntactic binder in scope returns that
        // binder node directly (the only "resolution" the structural lowerer
        // performs — a purely syntactic, in-scope, query-free lookup).
        // Otherwise the name is unresolved: lower its arguments structurally
        // and emit a `BareRef` carrier. NEVER a `DeclRef` / `InstantiationRef`
        // / host bare-name fallback / builtin shadowing / enum projection.
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            // Binder lookup runs BEFORE the type-arg gate so a shadowed name
            // never leaks past its binder. A bare bound name returns its binder
            // node directly; a bound name WITH type arguments is an applied
            // binder (`T<X>`) the query-free lowerer cannot represent without
            // resolution (there is no structural "apply args to a binder"
            // carrier), so it is a typed error — never a `BareRef` that would
            // leak the shadowed name as if it were an unbound reference.
            if let Some(binder) = ctx.lookup_binder(name) {
                if type_arguments.is_empty() {
                    return Ok(binder);
                }
                return Err(StructuralLowerError::UnsupportedWithoutResolution {
                    shape: "AppliedBinder",
                });
            }
            let type_args = lower_args(graph, type_arguments, scope, ctx)?;
            Ok(graph.intern_node_with_scope(
                SemanticNodeData::new_bare_ref(Arc::clone(name), scope.clone(), type_args),
                scope.clone(),
            ))
        }

        // -- Raw fallback (display/compat only, never a control signal) --
        TypeExpr::Unknown(value) => Ok(graph.intern_node_with_scope(
            SemanticNodeData::RawFallback {
                value: value.clone(),
            },
            scope.clone(),
        )),

        // -- Synthetic slot binding --
        // Identity is the content-free `SyntheticBindingId`; the carrier
        // key's `value_node` ordinal is value-side provenance carried
        // alongside, NOT folded into identity.
        TypeExpr::SyntheticSlotBinding(key) => Ok(graph.intern_node_with_scope(
            SemanticNodeData::SyntheticBinding {
                id: SyntheticBindingId::from_carrier_key(key),
                value_node: key.value_node,
            },
            scope.clone(),
        )),

        // -- `typeof value.path<args>` --
        // The value root is the FIRST path segment, keyed under the owner
        // scope; the remaining segments are the projected member path. The
        // root scope is the owner's lexical root — never a host lookup of
        // where the value actually resolves. Instantiation-expression args
        // are structurally lowered.
        TypeExpr::TypeOf(value_ref) => {
            let Some((root, rest)) = value_ref.path.split_first() else {
                return Err(StructuralLowerError::UnsupportedWithoutResolution {
                    shape: "TypeOf with empty value path",
                });
            };
            let value_root = ValueRootKey {
                scope: value_root_scope(scope)?,
                name: Arc::from(root.as_str()),
            };
            let path: Arc<[Arc<str>]> = rest.iter().map(|s| Arc::from(s.as_str())).collect();
            let type_args = lower_args(graph, &value_ref.type_args, scope, ctx)?;
            Ok(graph.intern_node_with_scope(
                SemanticNodeData::new_typeof(value_root, path, type_args),
                scope.clone(),
            ))
        }

        // -- Dynamic-import type carrier --
        // The specifier / qualifier / typeof flag pass through verbatim and
        // the type arguments are structurally lowered; module resolution is
        // never performed at lowering time.
        TypeExpr::ImportType {
            specifier,
            qualifier,
            typeof_query,
            type_arguments,
        } => {
            let type_args = lower_args(graph, type_arguments, scope, ctx)?;
            Ok(graph.intern_node_with_scope(
                SemanticNodeData::new_import_type(
                    Arc::clone(specifier),
                    Arc::clone(qualifier),
                    type_args,
                    *typeof_query,
                ),
                scope.clone(),
            ))
        }

        // -- Object surface --
        // Members carry their full declaration-site provenance verbatim
        // (spans / visibility / optional / readonly / method flags) plus the
        // owner-stamped `declaration_origin` (the object's lowering file),
        // `merge_role`, and macro-own-body bit. Member VALUES lower under
        // structural provenance (a nested object is not THIS object's
        // own-body / macro root); call / construct / index signatures lower
        // under the same context. Heritage is represented one level up as an
        // `Intersection` of a reference arm and the own-body object — never a
        // member of this object.
        TypeExpr::Object(obj) => {
            let declaration_origin = scope.canonical_file();
            let value_ctx = ctx.structural_provenance();
            let mut members: Vec<SurfaceMember> = Vec::new();
            let mut call_signatures: Vec<SemanticNodeId> = Vec::new();
            let mut construct_signatures: Vec<SemanticNodeId> = Vec::new();
            let mut index_signatures: Vec<IndexSignature> = Vec::new();
            for member in &obj.properties {
                match member {
                    ObjectMember::Property(prop) => members.push(SurfaceMember {
                        key: lower_authored_property_key(graph, &prop.key, scope, &value_ctx)?,
                        value: lower_node(graph, &prop.ty, scope, &value_ctx)?,
                        optional: prop.optional,
                        readonly: prop.readonly,
                        method_kind: None,
                        has_implementation_body: false,
                        visibility: prop.visibility,
                        excess_origin: prop.excess_origin,
                        spans: prop.spans,
                        declaration_origin: declaration_origin.clone(),
                        declared_in_macro_type_arg: ctx.macro_own_body,
                        merge_role: ctx.merge_role,
                    }),
                    ObjectMember::Method(method) => {
                        let function_expr = TypeExpr::Function(Arc::new(method.function.clone()));
                        register_structural_function_alias(
                            ctx.infer_binders
                                .expect("structural lowering injects infer identity"),
                            &function_expr,
                            &method.function,
                        );
                        members.push(SurfaceMember {
                            key: lower_authored_property_key(
                                graph,
                                &method.key,
                                scope,
                                &value_ctx,
                            )?,
                            value: lower_node(graph, &function_expr, scope, &value_ctx)?,
                            optional: method.optional,
                            readonly: false,
                            method_kind: Some(method.method_kind),
                            has_implementation_body: method.has_implementation_body,
                            visibility: method.visibility,
                            excess_origin: method.excess_origin,
                            spans: method.spans,
                            declaration_origin: declaration_origin.clone(),
                            declared_in_macro_type_arg: ctx.macro_own_body,
                            merge_role: ctx.merge_role,
                        });
                    }
                    ObjectMember::CallSignature(func) => {
                        let function_expr = TypeExpr::Function(Arc::new(func.clone()));
                        register_structural_function_alias(
                            ctx.infer_binders
                                .expect("structural lowering injects infer identity"),
                            &function_expr,
                            func,
                        );
                        call_signatures.push(lower_node(graph, &function_expr, scope, ctx)?);
                    }
                    ObjectMember::ConstructSignature(func) => {
                        let function_expr = TypeExpr::ConstructorType(Arc::new(func.clone()));
                        register_structural_function_alias(
                            ctx.infer_binders
                                .expect("structural lowering injects infer identity"),
                            &function_expr,
                            func,
                        );
                        construct_signatures.push(lower_node(graph, &function_expr, scope, ctx)?);
                    }
                    ObjectMember::IndexSignature(sig) => index_signatures.push(IndexSignature {
                        key_type: lower_node(graph, &sig.key_type, scope, ctx)?,
                        value_type: lower_node(graph, &sig.value_type, scope, ctx)?,
                        readonly: sig.readonly,
                        spans: sig.spans,
                        declaration_origin: declaration_origin.clone(),
                    }),
                    // A spread-bearing literal needs the dispatch-owned
                    // spread materializer's fold; this query-free lowerer
                    // fails closed — never a silently spread-less surface.
                    ObjectMember::Spread(_) => {
                        return Err(StructuralLowerError::UnsupportedWithoutResolution {
                            shape: "object-literal spread",
                        })
                    }
                }
            }
            let has_index_signature = !index_signatures.is_empty();
            let view = crate::semantic_query::SurfaceView::from_init(
                crate::semantic_query::SurfaceViewInit {
                    members: Arc::from(members.into_boxed_slice()),
                    call_signatures: Arc::from(call_signatures.into_boxed_slice()),
                    construct_signatures: Arc::from(construct_signatures.into_boxed_slice()),
                    index_signatures: Arc::from(index_signatures.into_boxed_slice()),
                    keyspace: None,
                    has_index_signature,
                },
            );
            Ok(graph.intern_node_with_scope(SemanticNodeData::Object(view), scope.clone()))
        }

        // -- Deferred mapped-type shell (NEVER materialized) --
        // The `[K in S]` binder is interned as a mapper `TypeParam` (with an
        // emission-only ordinal) and bound for the value / name-remap bodies;
        // a `keyof T` source unwraps to source = T, key space = `keyof T`
        // shell. The shell is always preserved — the per-key value surface is
        // never enumerated here.
        TypeExpr::Mapped {
            parameter,
            source,
            value,
            optional,
            readonly,
            name_type,
            ..
        } => {
            let mapper_display_name: Arc<str> = Arc::from(parameter.as_str());
            let mapper_decl = DeclIdentity::from_scope(scope, Arc::from("<mapper-param>"));
            let parameter_node = graph.intern_node_with_scope(
                SemanticNodeData::TypeParam {
                    decl: mapper_decl,
                    param_index: ctx.next_mapper_ordinal(),
                    constraint: None,
                    default: None,
                    display_name: Arc::clone(&mapper_display_name),
                },
                scope.clone(),
            );
            let (source_node, key_space, base_infer_name) = match source.as_ref() {
                TypeExpr::KeyOf(inner) => {
                    let inner_id = lower_node(graph, inner, scope, ctx)?;
                    let key_space = graph.intern_node_with_scope(
                        SemanticNodeData::KeyOf { base: inner_id },
                        scope.clone(),
                    );
                    let base_infer = match graph.node_data(inner_id).as_deref() {
                        Some(SemanticNodeData::Infer { name, binder }) => {
                            Some((Arc::clone(name), binder.clone()))
                        }
                        _ => None,
                    };
                    (inner_id, key_space, base_infer)
                }
                _ => {
                    let lowered = lower_node(graph, source, scope, ctx)?;
                    (lowered, lowered, None)
                }
            };

            // Seed a scoped reference only for the exact `Infer` declaration
            // selected by `keyof infer T`. The mapper frame is innermost so a
            // same-name `[T in ...]` binder capture-avoidingly shadows it.
            let mut frames: Vec<BinderScope> = ctx.binders.to_vec();
            if let Some((base_infer_name, binder)) = base_infer_name {
                let reference = graph.intern_node_with_scope(
                    SemanticNodeData::InferRef {
                        name: Arc::clone(&base_infer_name),
                        binder,
                    },
                    scope.clone(),
                );
                let mut base_infer_frame = BinderScope::default();
                base_infer_frame.bind(base_infer_name, reference);
                frames.push(base_infer_frame);
            }
            let mut mapper_frame = BinderScope::default();
            mapper_frame.bind(Arc::clone(&mapper_display_name), parameter_node);
            frames.push(mapper_frame);
            let body_ctx = ctx.with_binders(&frames);

            let value_expr = lower_node(graph, value, scope, &body_ctx)?;
            let name_remap = name_type
                .as_deref()
                .map(|nt| lower_node(graph, nt, scope, &body_ctx))
                .transpose()?;
            let optionality = match optional {
                MappedModifier::Add => OptionalityMod::Add,
                MappedModifier::Remove => OptionalityMod::Remove,
                _ => OptionalityMod::Keep,
            };
            let readonly = match readonly {
                MappedModifier::Add => ReadonlyMod::Add,
                MappedModifier::Remove => ReadonlyMod::Remove,
                _ => ReadonlyMod::Keep,
            };
            let kind =
                MapperKind::classify_value_expr(graph, value_expr, source_node, parameter_node);
            let mapper = MapperKey {
                parameter_node,
                key_space,
                value_expr,
                optionality,
                readonly,
                name_remap,
                kind,
            };
            Ok(graph.intern_node_with_scope(
                SemanticNodeData::Mapped {
                    source: source_node,
                    mapper,
                },
                scope.clone(),
            ))
        }
    }
}

fn lower_authored_property_key(
    graph: &SemanticGraphStore,
    key: &verter_type_expr::TypeAuthoredPropertyKey,
    scope: &NodeScopeId,
    ctx: &StructuralLowerContext<'_>,
) -> Result<AuthoredPropertyKey, StructuralLowerError> {
    Ok(match key {
        verter_type_expr::AuthoredPropertyKey::String(value) => {
            AuthoredPropertyKey::String(Arc::clone(value))
        }
        verter_type_expr::AuthoredPropertyKey::Number(value) => AuthoredPropertyKey::Number(*value),
        verter_type_expr::AuthoredPropertyKey::UniqueSymbol(identity) => {
            AuthoredPropertyKey::UniqueSymbol(identity.clone())
        }
        verter_type_expr::AuthoredPropertyKey::Computed(expression) => {
            AuthoredPropertyKey::Computed(lower_node(graph, expression, scope, ctx)?)
        }
    })
}
/// The value-root [`ScopeId`] for a `typeof` carrier — the owner file scope
/// with no inner local scope (mirroring the eager value-root construction).
/// A `typeof` in a scope-less (`Global`) context has no canonical file to
/// root the value lookup in, so it cannot be structurally lowered.
fn value_root_scope(scope: &NodeScopeId) -> Result<ScopeId, StructuralLowerError> {
    match scope {
        NodeScopeId::File {
            canonical_id,
            owner,
            ..
        } => Ok(ScopeId {
            canonical_id: Arc::clone(canonical_id),
            owner: *owner,
            local_scope: None,
            binder_scope_id: crate::semantic_query::BinderScopeId::file_scope(*owner),
        }),
        NodeScopeId::Global => Err(StructuralLowerError::UnsupportedWithoutResolution {
            shape: "TypeOf in a scope-less (Global) context",
        }),
    }
}

/// Lower a `Union` / `Intersection` arm list to its structural shell:
/// an empty list degenerates to `never`, a one-arm list unwraps to that
/// arm, and a multi-arm list interns the composite (matching the eager
/// structural shape). No set normalization (dedup / absorption) is
/// performed — that is a reduction.
fn lower_union_or_intersection(
    graph: &SemanticGraphStore,
    arms: &[TypeExpr],
    scope: &NodeScopeId,
    ctx: &StructuralLowerContext<'_>,
    is_union: bool,
) -> Result<SemanticNodeId, StructuralLowerError> {
    let ids = lower_args(graph, arms, scope, ctx)?;
    Ok(if ids.is_empty() {
        graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never))
    } else if ids.len() == 1 {
        ids[0]
    } else if is_union {
        graph.intern_node_with_scope(SemanticNodeData::Union(ids), scope.clone())
    } else {
        graph.intern_node_with_scope(SemanticNodeData::Intersection(ids), scope.clone())
    })
}

/// Lower a function / constructor signature to a `Function` node.
///
/// A function type's own `<T>` binders shadow outer generics: each is
/// interned as a `TypeParam` binder node and pushed onto a fresh binder
/// frame so param / return references to an own type-parameter resolve to
/// that binder rather than a `BareRef`. The generic head lowers
/// INCREMENTALLY to match TypeScript scoping: each type parameter's
/// constraint / default sees the PRIOR parameters in this list (so the `T`
/// in `<T, U extends T>` / `<T, U = T>` binds to the first own binder), but
/// not itself or any following parameter — the first parameter's head
/// therefore lowers under the OUTER context alone. An absent return
/// annotation mirrors the eager `Opaque(Miss)` placeholder.
fn lower_function_signature(
    graph: &SemanticGraphStore,
    func: &FunctionExpr,
    scope: &NodeScopeId,
    ctx: &StructuralLowerContext<'_>,
    kind: SignatureKind,
) -> Result<SemanticNodeId, StructuralLowerError> {
    let mut own_frame = BinderScope::default();
    let mut type_parameters: Vec<TypeParamDecl> = Vec::with_capacity(func.type_parameters.len());
    for tp in &func.type_parameters {
        let display_name: Arc<str> = Arc::from(tp.name.as_str());
        // This parameter's constraint / default sees the PRIOR own binders in
        // this list (TS scoping), so build a head context over the outer stack
        // plus the own-frame accumulated so far. The first parameter has no
        // prior own binders and lowers under the outer `ctx` directly
        // (`head_storage` outlives `head_ctx`'s borrow, mirroring the
        // `inner_storage` body shape below).
        let head_storage;
        let head_ctx = if type_parameters.is_empty() {
            *ctx
        } else {
            let mut frames: Vec<BinderScope> = ctx.binders.to_vec();
            frames.push(own_frame.clone());
            head_storage = frames;
            ctx.with_binders(&head_storage)
        };
        let constraint = tp
            .constraint
            .as_deref()
            .map(|c| lower_node(graph, c, scope, &head_ctx))
            .transpose()?;
        let default = tp
            .default
            .as_deref()
            .map(|d| lower_node(graph, d, scope, &head_ctx))
            .transpose()?;
        let decl = DeclIdentity::from_scope(scope, Arc::clone(&display_name));
        let binder = graph.intern_node_with_scope(
            SemanticNodeData::TypeParam {
                decl,
                param_index: 0,
                constraint,
                default,
                display_name: Arc::clone(&display_name),
            },
            scope.clone(),
        );
        own_frame.bind(Arc::clone(&display_name), binder);
        type_parameters.push(TypeParamDecl {
            name: display_name,
            constraint,
            default,
        });
    }

    // Extend the binder stack with the own-generic frame for the body
    // (`inner_storage` outlives `inner_ctx`'s borrow — the eager arm uses
    // the same conditional-storage shape).
    let inner_storage;
    let inner_ctx = if func.type_parameters.is_empty() {
        *ctx
    } else {
        let mut frames: Vec<BinderScope> = ctx.binders.to_vec();
        frames.push(own_frame);
        inner_storage = frames;
        ctx.with_binders(&inner_storage)
    };

    let mut params: Vec<FunctionParam> = Vec::with_capacity(func.parameters.len());
    for p in &func.parameters {
        params.push(FunctionParam {
            name: p.name.as_deref().map(Arc::<str>::from),
            ty: lower_node(graph, &p.ty, scope, &inner_ctx)?,
            optional: p.optional,
            rest: p.rest,
            span: p.span,
        });
    }
    let return_type = match func.return_type.as_deref() {
        Some(ret) => lower_node(graph, ret, scope, &inner_ctx)?,
        None => graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
    };
    Ok(graph.intern_node_with_scope(
        SemanticNodeData::Signature {
            kind,
            params: Arc::from(params.into_boxed_slice()),
            return_type,
            type_parameters: Arc::from(type_parameters.into_boxed_slice()),
            signature_span: func.spans.signature,
            return_type_span: func.spans.return_type,
        },
        scope.clone(),
    ))
}

/// Lower a slice of `TypeExpr` arguments structurally, in order, into the
/// interned-id slice carried by a reference / import-type carrier.
fn lower_args(
    graph: &SemanticGraphStore,
    args: &[TypeExpr],
    scope: &NodeScopeId,
    ctx: &StructuralLowerContext<'_>,
) -> Result<Arc<[SemanticNodeId]>, StructuralLowerError> {
    let mut lowered = Vec::with_capacity(args.len());
    for arg in args {
        lowered.push(lower_node(graph, arg, scope, ctx)?);
    }
    Ok(Arc::from(lowered.into_boxed_slice()))
}

fn register_structural_function_alias(
    infer_binders: &crate::semantic_query::InferBinderFactory,
    alias: &TypeExpr,
    original: &FunctionExpr,
) {
    let alias_function = match alias {
        TypeExpr::Function(function) | TypeExpr::ConstructorType(function) => function,
        _ => return,
    };
    for (alias_parameter, original_parameter) in alias_function
        .type_parameters
        .iter()
        .zip(&original.type_parameters)
    {
        if let (Some(alias), Some(original)) = (
            alias_parameter.constraint.as_deref(),
            original_parameter.constraint.as_deref(),
        ) {
            infer_binders.register_equivalent_subtree(alias, original);
        }
        if let (Some(alias), Some(original)) = (
            alias_parameter.default.as_deref(),
            original_parameter.default.as_deref(),
        ) {
            infer_binders.register_equivalent_subtree(alias, original);
        }
    }
    for (alias_parameter, original_parameter) in
        alias_function.parameters.iter().zip(&original.parameters)
    {
        infer_binders.register_equivalent_subtree(&alias_parameter.ty, &original_parameter.ty);
    }
    if let (Some(alias), Some(original)) = (
        alias_function.return_type.as_deref(),
        original.return_type.as_deref(),
    ) {
        infer_binders.register_equivalent_subtree(alias, original);
    }
}

// =============================================================================
// The `<script setup generic="…">` binder-seed builder (PRIVATE).
// =============================================================================

/// Build the seed [`BinderScope`] stack from the owner's script-setup type
/// bindings. Returns a one-frame stack (or an empty stack when there are no
/// script-setup generics). Each binder interns a
/// [`SemanticNodeData::TypeParam`] node matching the eager path's shape
/// (`<script-setup>` decl sentinel + the binding ordinal + lowered
/// constraint / default + display name). The constraint / default lower
/// under the seed frame accumulated SO FAR, so an earlier generic is visible
/// to a later one's constraint (`generic="T, U extends T">`) per TS scoping.
///
/// The `<script setup generic="…">` clause is re-sourced from the owner's
/// ROUTE-FREE local [`IndexedReady`] data (raw source + framework parse)
/// through the ONE artifact-local transient producer
/// [`indexed_script_setup_type_params`](crate::host_resolve::indexed_script_setup_type_params)
/// — the same clause ingress the dispatch's script-setup `TypeParam`
/// node construction uses, so the seed binder shape is identical. The
/// helper does NOT read the prepared-decl bundle (whose cold path can
/// route-resolve imports) — that would make the producer impure.
///
/// PRIVATE (no visibility modifier): an internal helper of
/// [`build_macro_hot_ref`], confined to this module. Each binder's constraint
/// / default lowers through [`lower_type_expr_structural`] DIRECTLY — the
/// binder-seed lowering is part of building the macro handle's scope, NOT a
/// second macro-arg producer.
fn build_script_setup_seed_frames(
    indexed: &crate::project_type_store::IndexedReady,
    graph: &SemanticGraphStore,
    scope: &NodeScopeId,
) -> Vec<BinderScope> {
    // Re-source the `<script setup generic="…">` clause from the owner's local
    // route-free parse artifact through the ONE transient producer. The
    // clause-position index IS the ordinal (the same `param_index` the eager
    // path / prepared-decl bundle assigns), so the interned `TypeParam`
    // identity tuple matches.
    let params = crate::host_resolve::indexed_script_setup_type_params(indexed);
    if params.is_empty() {
        return Vec::new();
    }

    let (canonical_id, owner, whole_hash) = match scope {
        NodeScopeId::Global => return Vec::new(),
        NodeScopeId::File {
            canonical_id,
            owner,
            whole_hash,
            ..
        } => (canonical_id, *owner, *whole_hash),
    };
    let decl = DeclIdentity {
        canonical_id: Arc::clone(canonical_id),
        owner,
        whole_hash,
        decl_name: Arc::from("<script-setup>"),
    };

    let mut frame = BinderScope::default();
    for (idx, param) in params.iter().enumerate() {
        // The constraint / default see the binders accumulated so far. Lower
        // through the private structural lowerer directly — the binder-seed
        // lowering is part of building the macro handle's scope, not a second
        // producer. A scoped non-allocating borrow of the current frame
        // (`std::slice::from_ref`) feeds the head context without cloning the
        // accumulated binder map per parameter.
        let head_frames = std::slice::from_ref(&frame);
        let bound_locator = |position| {
            verter_type_expr::locators::AuthoredBodyLocator::DeclBody(
                verter_type_expr::locators::TypeBodySlot {
                    anchor: verter_type_expr::locators::AuthoredAnchor {
                        canonical_id: Arc::clone(canonical_id),
                        owner,
                        symbol: Arc::from("<script-setup>"),
                        space: verter_type_expr::locators::LocatorSymbolSpace::Type,
                    },
                    path: Arc::from(
                        Vec::from([
                            verter_type_expr::locators::TypeBodyPathStep::TypeParamBound {
                                ordinal: u32::try_from(idx).unwrap_or(u32::MAX),
                                position,
                            },
                        ])
                        .into_boxed_slice(),
                    ),
                },
            )
        };
        let constraint = param.constraint.as_ref().and_then(|c| {
            let source =
                bound_locator(verter_type_expr::locators::TypeParamBoundPosition::Constraint);
            let head_ctx = StructuralLowerContext::new(head_frames).with_infer_source(&source);
            lower_type_expr_structural(graph, c, scope.clone(), &head_ctx)
                .ok()
                .map(HotTypeRef::node)
        });
        let default = param.default.as_ref().and_then(|d| {
            let source = bound_locator(verter_type_expr::locators::TypeParamBoundPosition::Default);
            let head_ctx = StructuralLowerContext::new(head_frames).with_infer_source(&source);
            lower_type_expr_structural(graph, d, scope.clone(), &head_ctx)
                .ok()
                .map(HotTypeRef::node)
        });
        // The clause-position index is the ordinal / `param_index` the eager
        // path and prepared-decl bundle assign — matching identity tuples.
        let ordinal = u16::try_from(idx).unwrap_or(u16::MAX);
        let display_name: Arc<str> = Arc::from(param.name.as_str());
        let node: SemanticNodeId = graph.intern_node_with_scope(
            SemanticNodeData::TypeParam {
                decl: decl.clone(),
                param_index: ordinal,
                constraint,
                default,
                display_name: Arc::clone(&display_name),
            },
            scope.clone(),
        );
        frame.bind(display_name, node);
    }

    // De-sugared `vec![frame]`: this producer module forbids ALL production bang
    // macro invocations (so the no-codegen-surface guard bans the whole class
    // rather than allowlisting std macros), so the single-element vector is built
    // via `Vec::from`. Behavior-identical to `vec![frame]` (one owned move into a
    // fresh `Vec`).
    Vec::from([frame])
}

/// One lazy macro hot-mirror read: the structural graph handle plus
/// graph-free authored reference heads keyed by analyzed prop-field index.
///
/// The sidecar is route evidence only. It never participates in semantic node
/// or query identity.
#[derive(Debug, Clone)]
pub(crate) struct MacroHotProduct {
    pub(crate) hot: HotTypeRef,
    pub(crate) prop_reference_heads:
        Arc<[Option<verter_type_expr::facts::AuthoredReferenceHeadFact>]>,
}

/// Lazy, singleflight storage for one file's macro type-argument handles.
#[derive(Default)]
pub(crate) struct MacroHotMirror {
    cells: OnceLock<Box<[MacroSlot]>>,
}

#[derive(Default)]
struct MacroSlot {
    committed: OnceLock<Option<Arc<MacroHotProduct>>>,
    build_lock: parking_lot::Mutex<()>,
}

impl MacroHotMirror {
    fn slot(&self, macro_count: usize, macro_index: usize) -> Option<MacroMirrorSlot<'_>> {
        let cells = self.cells.get_or_init(|| {
            (0..macro_count)
                .map(|_| MacroSlot::default())
                .collect::<Vec<_>>()
                .into_boxed_slice()
        });
        cells.get(macro_index).map(MacroMirrorSlot)
    }

    #[cfg(test)]
    fn demanded_count(&self) -> usize {
        self.cells.get().map_or(0, |cells| {
            cells
                .iter()
                .filter(|cell| cell.committed.get().is_some())
                .count()
        })
    }
}

#[derive(Clone, Copy)]
struct MacroMirrorSlot<'a>(&'a MacroSlot);

impl<'a> MacroMirrorSlot<'a> {
    fn committed(self) -> Option<Option<Arc<MacroHotProduct>>> {
        self.0.committed.get().cloned()
    }

    fn lock_build(self) -> MacroMirrorBuildGuard<'a> {
        MacroMirrorBuildGuard {
            slot: self.0,
            _lock: self.0.build_lock.lock(),
        }
    }
}

/// Opaque proof that the producer owns the cold-build lock for one exact slot.
struct MacroMirrorBuildGuard<'a> {
    slot: &'a MacroSlot,
    _lock: parking_lot::MutexGuard<'a, ()>,
}

impl MacroMirrorBuildGuard<'_> {
    fn commit(self, result: Option<Arc<MacroHotProduct>>) {
        let _ = self.slot.committed.set(result);
    }
}

impl std::fmt::Debug for MacroHotMirror {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let demanded = self.cells.get().map_or(0, |cells| {
            cells
                .iter()
                .filter(|cell| cell.committed.get().is_some())
                .count()
        });
        f.debug_struct("MacroHotMirror")
            .field("demanded", &demanded)
            .finish()
    }
}

impl Clone for MacroHotMirror {
    fn clone(&self) -> Self {
        Self {
            cells: OnceLock::new(),
        }
    }
}

/// Resolve (lowering once on first demand) the mode-NEUTRAL
/// [`MacroHotProduct`] for the macro at `macro_index` in `owner_canonical`.
///
/// This is the SOLE production entry that lowers a macro
/// `parsed_type_argument` into a semantic-graph handle and projects its
/// graph-free prop-reference-head sidecar from the same typed-IR borrow.
/// Returns `None` when the owner file is not loaded, the macro index is out of
/// range, the macro carries no `parsed_type_argument`, or the type argument
/// has no faithful unresolved structural representation (a stable negative
/// cell).
pub(crate) fn macro_type_arg_hot_ref(
    ctx: &dyn ResolverContext,
    owner_canonical: &str,
    macro_index: usize,
) -> Option<MacroHotProduct> {
    macro_hot_product(ctx, owner_canonical, macro_index).map(|product| product.as_ref().clone())
}

fn macro_hot_product(
    ctx: &dyn ResolverContext,
    owner_canonical: &str,
    macro_index: usize,
) -> Option<Arc<MacroHotProduct>> {
    let serve = ctx.ensure_indexed_ready_serve(owner_canonical)?;
    let indexed = serve.indexed;

    // Lazily allocate the dense cell table once, sized to the owner's macro
    // count (race-safe via the outer `OnceLock::get_or_init`). An
    // out-of-range `macro_index` returns `None` (same negative as a missing
    // macro), never grows the table.
    let macro_count = indexed
        .script_analysis
        .as_ref()
        .map(|script| script.macros.len())
        .unwrap_or(0);
    let mirror = &indexed.macro_hot_mirror;
    let cell = mirror.slot(macro_count, macro_index)?;

    // The mirror is a PURE producer of the UNRESOLVED structural carrier graph
    // (inert carrier nodes, resolved on demand at the consuming dispatch):
    // no host route lookup, no dependency emission. Dependency recording
    // belongs at the RESOLVING demand — the consumer re-enters the ONE
    // dispatch over this handle and the subquery read signatures (`TypeOf`
    // import-route facts, `ResolveDecl`/`Instantiate` file whole-hashes)
    // bubble into the consuming result's `ReadSetSignature`.
    //
    // Preserve the typed lowering outcome across the write-once mirror-slot
    // admission: a transient broken decl-body lease (`LeaseMiss`) must NOT be
    // committed as a permanent negative — leave the slot VACANT, mark the
    // generalized non-cacheability rail, and let the next demand retry. Only a
    // built ref OR a genuine (cacheable) absence commits.
    //
    // Lock-free warm read first.
    if let Some(committed) = cell.committed() {
        return committed;
    }
    // Test-only rendezvous between the lock-free warm MISS and the build lock: when
    // armed it holds every thread that has just missed until ALL of them have, which
    // is the precise interleaving the per-slot build lock exists to serialise (see
    // `TestForceKnobs::macro_hot_post_warm_miss_barrier`, whose NON-RE-ENTRANCY
    // invariant this cold path upholds: the builder below produces inert carrier
    // nodes and resolves nothing, so it never re-enters this demand).
    #[cfg(test)]
    ctx.host_for_fact_tracer_install()
        .test_force
        .wait_macro_hot_post_warm_miss_barrier();
    // Cold path: SINGLEFLIGHT the lowering under the per-slot build lock so
    // concurrent first demands of ONE macro collapse onto a single
    // `build_macro_hot_ref` (the `OnceLock` alone cannot serialize — a
    // `LeaseMiss` leaves the slot vacant for retry, which forbids
    // `get_or_init`). Re-check under the lock: a racing builder may have
    // committed while this thread waited on the lock.
    let build_guard = cell.lock_build();
    if let Some(committed) = cell.committed() {
        return committed;
    }
    match build_macro_hot_ref(ctx, owner_canonical, &indexed, macro_index) {
        MacroHotRefOutcome::Ready(result) => {
            // First-writer commit under the build lock: `set` cannot race a
            // second committer (all commits take this lock), so it succeeds and
            // `result` IS the committed value.
            build_guard.commit(result.clone());
            result
        }
        MacroHotRefOutcome::LeaseMiss => {
            // Transient broken lease: leave the slot VACANT (the build lock is
            // released on scope exit, so the next demand re-enters the cold path
            // and retries), and mark the generalized non-cacheability rail.
            crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                crate::resolver_core::resolver_context::NonCacheableReadReason::LeaseMiss,
            );
            None
        }
    }
}

/// Outcome of [`build_macro_hot_ref`]: a committable result (a built
/// [`HotTypeRef`] OR a genuine, cacheable absence) versus a TRANSIENT broken
/// decl-body lease pin (`LeaseMiss`). Kept distinct so the mirror-slot
/// admission commits `Ready` but leaves the slot VACANT on `LeaseMiss` (a
/// later live-lease demand retries), never persisting a transient negative.
enum MacroHotRefOutcome {
    Ready(Option<Arc<MacroHotProduct>>),
    LeaseMiss,
}

/// Build the structural [`HotTypeRef`] for one macro index — the
/// mirror-slot cold-compute body. Lowers the macro's `parsed_type_argument`
/// once through the shared query-free structural lowerer under a script-setup
/// seed binder frame. Returns a [`MacroHotRefOutcome`] so the caller can
/// distinguish a committable result from a transient broken-lease miss.
///
/// PRIVATE: it names [`lower_type_expr_structural`] and
/// [`build_script_setup_seed_frames`] DIRECTLY — both are module-private, so
/// only this module's own producer paths can reach them.
fn build_macro_hot_ref(
    ctx: &dyn ResolverContext,
    owner_canonical: &str,
    indexed: &crate::project_type_store::IndexedReady,
    macro_index: usize,
) -> MacroHotRefOutcome {
    // Singleflight probe: count each COLD build entry. The per-slot build lock
    // must collapse concurrent first demands of one macro onto ONE entry.
    #[cfg(test)]
    ctx.host_for_fact_tracer_install()
        .macro_hot_lowering_count
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // Genuine, cacheable absences (no script analysis, no macro at the index,
    // no authored type argument): commit `Ready(None)`.
    let Some(snapshot) = indexed.script_analysis.as_ref() else {
        return MacroHotRefOutcome::Ready(None);
    };
    let Some(mac) = snapshot.macros.get(macro_index) else {
        return MacroHotRefOutcome::Ready(None);
    };
    // The analyzer records the authored type-argument POSITION (a content-free
    // locator); the typed IR hydrates transiently from the memo's retained
    // snapshot at the macro call's span — the mirror is the position's sole
    // producer, and the lease-only re-borrow never re-parses.
    let Some(payload_locator) = mac.parsed_type_argument.as_ref() else {
        return MacroHotRefOutcome::Ready(None);
    };
    let parsed_arg = match indexed
        .shallow_state
        .decl_bodies()
        .transient_macro_type_argument(mac.span)
    {
        crate::decl_body_memo::DemandOutcome::Ready(Some(expr)) => expr,
        // A genuine, cacheable absence: the position lowered to no argument.
        crate::decl_body_memo::DemandOutcome::Ready(None) => {
            return MacroHotRefOutcome::Ready(None)
        }
        // A TRANSIENT broken decl-body lease: DO NOT commit a permanent
        // negative — surface the distinct outcome so the caller leaves the
        // mirror slot vacant, marks non-cacheability, and retries later.
        crate::decl_body_memo::DemandOutcome::LeaseMiss => return MacroHotRefOutcome::LeaseMiss,
    };
    let parsed_arg = parsed_arg.as_ref();
    let prop_reference_heads = mac
        .prop_fields
        .iter()
        .map(|field| {
            let payload = field.payload.as_ref()?;
            let ty = inline_macro_object_property_type(parsed_arg, field.name.as_str())?;
            Some(verter_semantic::analysis::macro_payload_reference_head_fact(ty, payload))
        })
        .collect::<Vec<_>>();

    let graph = ctx.project_type_store().semantic_graph();
    let scope = NodeScopeId::File {
        canonical_id: Arc::from(owner_canonical),
        owner: mac.owner,
        whole_hash: indexed.whole_hash,
        local_scope: None,
    };

    // Macro-T own-body provenance: `defineProps` / `withDefaults` own-body
    // direct members carry `declared_in_macro_type_arg = true` (a props-axis
    // concern consumed by the published surface policy). Every other macro is
    // structural. This mirrors `macro_payload_surface_provenance` —
    // PROVENANCE is a structural-lowering property of the macro's own body,
    // not a demand/mode property, so it belongs on the mode-neutral mirror.
    // The parse-domain producer owns the kind classification
    // (`defineProps` / `withDefaults` own bodies carry the bit).
    let macro_own_body = MacroOwnBodyStamp::from_macro_kind(mac.kind);

    // Seed the script-setup generic binders so `defineProps<T>()`'s `T` in a
    // `<script setup generic="T">` SFC lowers to its `TypeParam` binder, not
    // a `BareRef(T)`. Built from the owner's ROUTE-FREE local `IndexedReady`
    // data (`raw_source` + `framework_parse`) — NO host route lookup, so the
    // mirror stays a pure producer.
    let seed_frames = build_script_setup_seed_frames(indexed, graph, &scope);
    let infer_source =
        verter_type_expr::locators::AuthoredBodyLocator::MacroPayload(payload_locator.clone());
    let lower_ctx = StructuralLowerContext::new(&seed_frames)
        .with_macro_own_body(macro_own_body)
        .with_infer_source(&infer_source);

    // A lowering failure is a genuine (cacheable) absence — commit `Ready(None)`.
    MacroHotRefOutcome::Ready(
        lower_type_expr_structural(graph, parsed_arg, scope, &lower_ctx)
            .ok()
            .map(|hot| {
                Arc::new(MacroHotProduct {
                    hot,
                    prop_reference_heads: Arc::from(prop_reference_heads),
                })
            }),
    )
}

/// Find a direct named property in the hydrated inline macro object. This is
/// intentionally a tiny graph-free projection over the already-borrowed typed
/// IR: it neither resolves aliases nor walks another declaration body.
fn inline_macro_object_property_type<'a>(
    mut parsed_arg: &'a TypeExpr,
    field_name: &str,
) -> Option<&'a TypeExpr> {
    while let TypeExpr::Parenthesized(inner) = parsed_arg {
        parsed_arg = inner.as_ref();
    }
    let TypeExpr::Object(object) = parsed_arg else {
        return None;
    };
    object.properties.iter().find_map(|member| match member {
        ObjectMember::Property(property) if property.key.as_string() == Some(field_name) => {
            Some(&property.ty)
        }
        _ => None,
    })
}

#[cfg(test)]
#[path = "structural_lower_tests.rs"]
mod structural_lower_tests;

#[cfg(test)]
#[path = "macro_hot_mirror_tests.rs"]
mod macro_hot_mirror_tests;

#[cfg(test)]
#[path = "script_setup_binder_tests.rs"]
mod script_setup_binder_tests;
