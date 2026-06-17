//! Session-owned, query-free **structural** lowering of an owned
//! [`TypeExpr`] into the dormant semantic-graph carriers.
//!
//! This is the forward boundary that EMITS the unresolved carriers
//! (`BareRef`, `ImportType`, `RawFallback`, `ConstructorType`,
//! `SyntheticBinding`, and the tuple-element `rest` flag) alongside the
//! structural shells (objects, functions, unions, intersections, tuples,
//! operators) from the [`TypeExpr`] the OXC worker produces. It performs
//! NO name / import / type resolution or reduction:
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
//! these carriers; this lowerer never anticipates them.
//!
//! The lowerer is graph-local: it reaches the [`SemanticGraphStore`] to intern
//! the structural nodes it builds and read them back (e.g. the distributive
//! check / mapper classification); the owner supplies the rooting
//! [`NodeScopeId`]. It never touches `ProjectSemanticDispatch`, a resolver
//! context, a `SemanticQueryKey`, or any host / type-provider state — the
//! `session_graph_lowerer_makes_no_query` guard locks that statically.

// The query-free structural lowerer is a standalone producer of the
// unresolved graph carriers, exercised end-to-end by the test suite below.
// `dead_code` is allowed because the lowerer is dormant: it has no production
// caller yet; its eventual demand-time consumer (carrier resolution) lands as
// later carrier-resolution work.
#![allow(dead_code)]

use std::cell::Cell;
use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_type_expr::{FunctionExpr, LiteralValue, MappedModifier, ObjectMember, TypeExpr};

use crate::semantic_query::{
    DeclIdentity, FunctionParam, HotTypeRef, IndexKey, IndexSignature, MapperKey, MapperKind,
    MemberMergeRole, NodeScopeId, OptionalityMod, PrimitiveKind, QueryError, ReadonlyMod, ScopeId,
    SemanticNodeData, SemanticNodeId, SurfaceMember, SurfaceView, SyntheticBindingId, TupleElement,
    TypeParamDecl, ValueRootKey,
};
use crate::semantic_query_memo::SemanticGraphStore;

/// A lexical binder frame: the syntactic type-parameter / `infer` /
/// mapped-type-parameter names in scope at one nesting level, each mapped
/// to the already-interned binder node (a `TypeParam` or `Infer`
/// [`SemanticNodeId`]) it stands for.
///
/// A `Ref` whose name hits a binder returns that binder node directly
/// instead of emitting a `BareRef` — the only "resolution" the structural
/// lowerer performs is this purely syntactic, in-scope binder lookup, which
/// needs no host query.
#[derive(Debug, Default, Clone)]
pub(crate) struct BinderScope {
    names: FxHashMap<Arc<str>, SemanticNodeId>,
}

impl BinderScope {
    /// Bind a syntactic type-parameter name to its interned binder node.
    pub(crate) fn bind(&mut self, name: Arc<str>, node: SemanticNodeId) {
        self.names.insert(name, node);
    }

    /// The binder node a `name` stands for in this frame, if any.
    fn lookup(&self, name: &str) -> Option<SemanticNodeId> {
        self.names.get(name).copied()
    }
}

/// Structural / provenance inputs to the query-free lowerer.
///
/// This context carries ONLY syntactic and surface-provenance information:
/// the innermost-last stack of lexical [`BinderScope`] frames plus the
/// surface-merge role / macro-own-body provenance stamped onto object
/// members. It deliberately does NOT hold `ProjectSemanticDispatch`, a host
/// object, type-provider state, a `SemanticQueryKey`, or a
/// `ProjectionReductionContext` — the structural lowerer is not a resolver,
/// so it has no use for any demand-time resolution surface.
#[derive(Debug, Clone, Copy)]
pub(crate) struct StructuralLowerContext<'a> {
    /// Innermost-last stack of lexical binder frames; binder lookup scans
    /// from the top (last) frame outward so an inner type-parameter shadows
    /// an outer one of the same name.
    binders: &'a [BinderScope],
    /// Surface-merge role stamped on the DIRECT members of an object lowered
    /// under this context — `OwnBody` for an interface/class own body arm,
    /// `Heritage` for a heritage arm, `Authored` (the default) otherwise.
    /// Orthogonal to the macro-own-body axis.
    merge_role: MemberMergeRole,
    /// Whether the object lowered under this context is the macro
    /// type-argument's own body (sets `declared_in_macro_type_arg` on its
    /// direct members).
    macro_own_body: bool,
    /// Per-lowering allocator for mapped-type binder ordinals. The eager
    /// path keys these through the host-owned `MapperBinderRegistry` for
    /// cross-lowering cache stability; the query-free lowerer cannot reach
    /// host state, so it allocates emission-only ordinals from this counter
    /// — distinct `[K in …]` binders in one lowering get distinct ordinals.
    /// `None` outside a lowering (a freshly constructed root context);
    /// [`lower_type_expr_structural`] injects it.
    mapper_ordinals: Option<&'a Cell<u16>>,
}

impl<'a> StructuralLowerContext<'a> {
    /// A root context over `binders` (innermost last) with default
    /// provenance: an `Authored` merge role and not-a-macro-own-body. The
    /// empty slice is the no-binders-in-scope root.
    pub(crate) fn new(binders: &'a [BinderScope]) -> Self {
        Self {
            binders,
            merge_role: MemberMergeRole::Authored,
            macro_own_body: false,
            mapper_ordinals: None,
        }
    }

    /// Inject the per-lowering mapped-binder ordinal counter (called once at
    /// the lowering entry point so a caller never has to supply one).
    fn with_mapper_ordinals(mut self, ordinals: &'a Cell<u16>) -> Self {
        self.mapper_ordinals = Some(ordinals);
        self
    }

    /// The next mapped-binder ordinal for this lowering, or `0` when no
    /// counter is in scope (a root context constructed directly in a test).
    fn next_mapper_ordinal(&self) -> u16 {
        match self.mapper_ordinals {
            Some(cell) => {
                let next = cell.get();
                cell.set(next.saturating_add(1));
                next
            }
            None => 0,
        }
    }

    /// Replace the surface-merge role (the owner stamps `OwnBody` on an
    /// interface/class own-body arm and `Heritage` on a heritage arm).
    pub(crate) fn with_merge_role(mut self, merge_role: MemberMergeRole) -> Self {
        self.merge_role = merge_role;
        self
    }

    /// Mark whether this context lowers the macro type-argument's own body.
    pub(crate) fn with_macro_own_body(mut self, macro_own_body: bool) -> Self {
        self.macro_own_body = macro_own_body;
        self
    }

    /// Swap the binder stack, preserving the surface provenance and the
    /// mapper-ordinal counter (used when a function's own generics extend the
    /// stack for its body).
    fn with_binders<'b>(&self, binders: &'b [BinderScope]) -> StructuralLowerContext<'b>
    where
        'a: 'b,
    {
        StructuralLowerContext {
            binders,
            merge_role: self.merge_role,
            macro_own_body: self.macro_own_body,
            mapper_ordinals: self.mapper_ordinals,
        }
    }

    /// Downgrade for a nested member VALUE: a nested object inside a member
    /// type is not THIS object's macro own-body, but the merge-role axis is
    /// orthogonal and preserved (mirrors `into_structural_provenance`).
    fn structural_provenance(&self) -> Self {
        Self {
            binders: self.binders,
            merge_role: self.merge_role,
            macro_own_body: false,
            mapper_ordinals: self.mapper_ordinals,
        }
    }

    /// The binder node a `name` stands for, scanning from the innermost
    /// (last) frame outward so an inner type-parameter shadows an outer one.
    /// `None` when `name` is not a bound syntactic type-parameter — the
    /// caller then emits a `BareRef` carrier.
    fn lookup_binder(&self, name: &str) -> Option<SemanticNodeId> {
        self.binders
            .iter()
            .rev()
            .find_map(|frame| frame.lookup(name))
    }
}

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
pub(crate) enum StructuralLowerError {
    /// `shape` names the offending `TypeExpr` variant for diagnostics.
    UnsupportedWithoutResolution { shape: &'static str },
}

/// Structurally lower an owned [`TypeExpr`] into the dormant semantic-graph
/// carriers, rooted at the owner-supplied `scope`, performing no resolution.
///
/// Returns a [`HotTypeRef`] wrapping the interned root node, or a typed
/// [`StructuralLowerError`] for a shape with no unresolved representation.
pub(crate) fn lower_type_expr_structural(
    graph: &SemanticGraphStore,
    expr: &TypeExpr,
    scope: NodeScopeId,
    ctx: &StructuralLowerContext<'_>,
) -> Result<HotTypeRef, StructuralLowerError> {
    // The mapped-binder ordinal counter is per-lowering, owned here so a
    // caller never has to supply one.
    let mapper_ordinals = Cell::new(0u16);
    let ctx = ctx.with_mapper_ordinals(&mapper_ordinals);
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
            SemanticNodeData::Primitive(super::map_primitive_name(*name)),
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
                    match crate::project_semantic_dispatch::build::integer_convention_index_key(*n)
                    {
                        Some(i) => IndexKey::Number(i),
                        None => IndexKey::TypeNode(lower_node(graph, index, scope, ctx)?),
                    }
                }
                _ => IndexKey::TypeNode(lower_node(graph, index, scope, ctx)?),
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
            let mut infer_names: Vec<Arc<str>> = Vec::new();
            collect_extends_infer_binder_names(extends, &mut infer_names);
            let extends = lower_node(graph, extends, scope, ctx)?;
            let true_branch_ref = if infer_names.is_empty() {
                lower_node(graph, true_type, scope, ctx)?
            } else {
                let mut infer_frame = BinderScope::default();
                for name in &infer_names {
                    let infer_node = graph.intern_node_with_scope(
                        SemanticNodeData::Infer {
                            name: Arc::clone(name),
                        },
                        scope.clone(),
                    );
                    infer_frame.bind(Arc::clone(name), infer_node);
                }
                let mut frames: Vec<BinderScope> = ctx.binders.to_vec();
                frames.push(infer_frame);
                let true_ctx = ctx.with_binders(&frames);
                lower_node(graph, true_type, scope, &true_ctx)?
            };
            let false_branch_ref = lower_node(graph, false_type, scope, ctx)?;
            let distributive = matches!(
                graph.node_data(check).as_deref(),
                Some(SemanticNodeData::TypeParam { .. })
            );
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
        TypeExpr::Infer { name } => Ok(graph.intern_node_with_scope(
            SemanticNodeData::Infer {
                name: Arc::from(name.as_str()),
            },
            scope.clone(),
        )),

        // -- Function / constructor signatures --
        // A plain function lowers to the signature node directly; a
        // constructor type wraps the SAME signature in a `ConstructorType`
        // carrier so `new () => R` stays distinct from `() => R` (the eager
        // path flattens both to `Function` — this is the intentional
        // divergence the carrier exists for).
        TypeExpr::Function(func) => lower_function_signature(graph, func, scope, ctx),
        TypeExpr::ConstructorType(func) => {
            let signature = lower_function_signature(graph, func, scope, ctx)?;
            Ok(graph.intern_node_with_scope(
                SemanticNodeData::ConstructorType { signature },
                scope.clone(),
            ))
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
        TypeExpr::Unknown { raw } => Ok(graph.intern_node_with_scope(
            SemanticNodeData::RawFallback {
                raw: Arc::from(raw.as_str()),
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
                        name: Arc::from(prop.name.as_str()),
                        value: lower_node(graph, &prop.ty, scope, &value_ctx)?,
                        optional: prop.optional,
                        readonly: prop.readonly,
                        is_method: false,
                        visibility: prop.visibility,
                        spans: prop.spans,
                        declaration_origin: declaration_origin.clone(),
                        declared_in_macro_type_arg: ctx.macro_own_body,
                        merge_role: ctx.merge_role,
                    }),
                    ObjectMember::Method(method) => {
                        let function_expr = TypeExpr::Function(Arc::new(method.function.clone()));
                        members.push(SurfaceMember {
                            name: Arc::from(method.name.as_str()),
                            value: lower_node(graph, &function_expr, scope, &value_ctx)?,
                            optional: method.optional,
                            readonly: false,
                            is_method: true,
                            visibility: method.visibility,
                            spans: method.spans,
                            declaration_origin: declaration_origin.clone(),
                            declared_in_macro_type_arg: ctx.macro_own_body,
                            merge_role: ctx.merge_role,
                        });
                    }
                    ObjectMember::CallSignature(func) => {
                        let function_expr = TypeExpr::Function(Arc::new(func.clone()));
                        call_signatures.push(lower_node(graph, &function_expr, scope, ctx)?);
                    }
                    ObjectMember::ConstructSignature(func) => {
                        let function_expr = TypeExpr::Function(Arc::new(func.clone()));
                        construct_signatures.push(lower_node(graph, &function_expr, scope, ctx)?);
                    }
                    ObjectMember::IndexSignature(sig) => index_signatures.push(IndexSignature {
                        key_type: lower_node(graph, &sig.key_type, scope, ctx)?,
                        value_type: lower_node(graph, &sig.value_type, scope, ctx)?,
                        readonly: sig.readonly,
                        spans: sig.spans,
                        declaration_origin: declaration_origin.clone(),
                    }),
                }
            }
            let has_index_signature = !index_signatures.is_empty();
            let view = SurfaceView {
                members: Arc::from(members.into_boxed_slice()),
                call_signatures: Arc::from(call_signatures.into_boxed_slice()),
                construct_signatures: Arc::from(construct_signatures.into_boxed_slice()),
                index_signatures: Arc::from(index_signatures.into_boxed_slice()),
                keyspace: None,
                has_index_signature,
            };
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
            let mut mapper_frame = BinderScope::default();
            mapper_frame.bind(Arc::clone(&mapper_display_name), parameter_node);
            let mut frames: Vec<BinderScope> = ctx.binders.to_vec();
            frames.push(mapper_frame);
            let body_ctx = ctx.with_binders(&frames);

            let (source_node, key_space) = match source.as_ref() {
                TypeExpr::KeyOf(inner) => {
                    let inner_id = lower_node(graph, inner, scope, ctx)?;
                    let key_space = graph.intern_node_with_scope(
                        SemanticNodeData::KeyOf { base: inner_id },
                        scope.clone(),
                    );
                    (inner_id, key_space)
                }
                _ => {
                    let lowered = lower_node(graph, source, scope, ctx)?;
                    (lowered, lowered)
                }
            };
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

/// Collect the `infer` binder names introduced by a conditional's `extends`
/// clause (at any structural depth) into `out`, so the conditional's TRUE
/// branch can bind each to the matching `Infer` carrier. Purely syntactic
/// typed-IR walk — no resolution, allocation-free except the de-duplicated
/// name push.
///
/// This descends EVERY `TypeExpr` child position where an `infer` is
/// syntactically valid inside an `extends` clause, reaching at least the
/// composite coverage of the eager binder
/// [`ProjectSemanticDispatch::collect_infer_bindings_into_env`] (`Function` /
/// `Object` param/return/member positions) so the dormant carrier binds the
/// same names the eager path would: `Function` / `ConstructorType`
/// (parameters, return, own type-parameter constraint/default), `Object`
/// (property values, index-signature key/value, call/construct/method
/// signatures), `TemplateLiteral` interpolations, `Ref` / `ImportType` type
/// arguments, `TypeOf` instantiation arguments, and `Mapped` source / value /
/// `as`-remap name-type.
///
/// ONE deliberate non-descent: it does NOT recurse into a nested
/// `Conditional`, because an `infer` in an inner conditional's `extends` is
/// scoped to THAT conditional's true branch, not this one's — matching the
/// eager binder, which likewise has no `Conditional` arm.
fn collect_extends_infer_binder_names(expr: &TypeExpr, out: &mut Vec<Arc<str>>) {
    match expr {
        TypeExpr::Infer { name } => {
            if !out.iter().any(|n| n.as_ref() == name.as_str()) {
                out.push(Arc::from(name.as_str()));
            }
        }
        TypeExpr::Parenthesized(inner) | TypeExpr::Rest(inner) | TypeExpr::KeyOf(inner) => {
            collect_extends_infer_binder_names(inner, out)
        }
        TypeExpr::Array { element, .. } => collect_extends_infer_binder_names(element, out),
        TypeExpr::Union(arms) | TypeExpr::Intersection(arms) => arms
            .iter()
            .for_each(|a| collect_extends_infer_binder_names(a, out)),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .for_each(|e| collect_extends_infer_binder_names(&e.ty, out)),
        // A generic reference (`Box<infer U>`) and an import-type reference
        // (`import("m").Box<infer U>`) carry their `infer`s in the identical
        // `type_arguments` slice.
        TypeExpr::Ref { type_arguments, .. } | TypeExpr::ImportType { type_arguments, .. } => {
            type_arguments
                .iter()
                .for_each(|a| collect_extends_infer_binder_names(a, out))
        }
        TypeExpr::IndexedAccess { object, index } => {
            collect_extends_infer_binder_names(object, out);
            collect_extends_infer_binder_names(index, out);
        }
        // `infer` is valid in any parameter / return / own-type-parameter
        // position of a function or constructor type written inside the
        // `extends` clause (`T extends (x: infer P) => any ? P : …`,
        // `T extends new (x: infer P) => any ? P : …`).
        TypeExpr::Function(func) | TypeExpr::ConstructorType(func) => {
            collect_function_infer_binder_names(func, out)
        }
        // Object type literal: each property value, index-signature key/value,
        // and call / construct / method signature can carry an extends-clause
        // `infer` (`T extends { a: infer P } ? P : …`).
        TypeExpr::Object(obj) => {
            for member in &obj.properties {
                match member {
                    ObjectMember::Property(prop) => {
                        collect_extends_infer_binder_names(&prop.ty, out)
                    }
                    ObjectMember::IndexSignature(sig) => {
                        collect_extends_infer_binder_names(&sig.key_type, out);
                        collect_extends_infer_binder_names(&sig.value_type, out);
                    }
                    ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                        collect_function_infer_binder_names(func, out)
                    }
                    ObjectMember::Method(method) => {
                        collect_function_infer_binder_names(&method.function, out)
                    }
                }
            }
        }
        // Template-literal interpolations:
        // `` T extends `${infer Head}${string}` ? Head : … ``.
        TypeExpr::TemplateLiteral { expressions, .. } => expressions
            .iter()
            .for_each(|e| collect_extends_infer_binder_names(e, out)),
        // `typeof` instantiation-expression type arguments
        // (`typeof make<infer P>`) carry extends-clause infer binders.
        TypeExpr::TypeOf(value_ref) => value_ref
            .type_args
            .iter()
            .for_each(|a| collect_extends_infer_binder_names(a, out)),
        // Mapped type: the `in` source (the constraint), the value type, AND
        // the `as` remap (`name_type`) can each carry an extends-clause
        // `infer` (`T extends { [K in S as infer R]: V } ? R : …`). The mapper
        // parameter name itself is not an `infer` binder. Descending the
        // `name_type` keeps the collector's Mapped coverage a SUPERSET of the
        // eager binder's — the correct structural fidelity for the carrier
        // graph; an `infer` in the remap must bind for the true branch instead
        // of leaking as a `BareRef`.
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            collect_extends_infer_binder_names(source, out);
            collect_extends_infer_binder_names(value, out);
            if let Some(name_type) = name_type {
                collect_extends_infer_binder_names(name_type, out);
            }
        }
        // A nested conditional's `infer`s belong to ITS OWN true branch — do
        // not descend (TS scoping). The remaining terminals (`Primitive`,
        // `Literal`, `TypeParameter`, `RecursiveRef`, `SyntheticSlotBinding`,
        // `Unknown`) introduce no extends-clause infer binder here.
        _ => {}
    }
}

/// Collect extends-clause `infer` binder names appearing in any parameter
/// type, the return type, or any own type-parameter constraint / default of a
/// function or constructor signature. Shared by the `Function` /
/// `ConstructorType` arm and the object call / construct / method-signature
/// members of [`collect_extends_infer_binder_names`]. Purely syntactic — no
/// resolution.
fn collect_function_infer_binder_names(func: &FunctionExpr, out: &mut Vec<Arc<str>>) {
    for param in &func.parameters {
        collect_extends_infer_binder_names(&param.ty, out);
    }
    if let Some(return_type) = &func.return_type {
        collect_extends_infer_binder_names(return_type, out);
    }
    for tp in &func.type_parameters {
        if let Some(constraint) = &tp.constraint {
            collect_extends_infer_binder_names(constraint, out);
        }
        if let Some(default) = &tp.default {
            collect_extends_infer_binder_names(default, out);
        }
    }
}

/// The value-root [`ScopeId`] for a `typeof` carrier — the owner file scope
/// with no inner local scope (mirroring the eager value-root construction).
/// A `typeof` in a scope-less (`Global`) context has no canonical file to
/// root the value lookup in, so it cannot be structurally lowered.
fn value_root_scope(scope: &NodeScopeId) -> Result<ScopeId, StructuralLowerError> {
    match scope {
        NodeScopeId::File { canonical_id, .. } => Ok(ScopeId {
            canonical_id: Arc::clone(canonical_id),
            local_scope: None,
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
        SemanticNodeData::Function {
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

#[cfg(test)]
#[path = "structural_lower_tests.rs"]
mod structural_lower_tests;
