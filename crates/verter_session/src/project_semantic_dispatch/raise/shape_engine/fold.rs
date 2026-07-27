//! The single shared `SemanticNodeData` fold and its folded-intermediate
//! structs, split from the parent `shape_engine` module for file-size. The
//! fold's control flow (the `?` aborts, presence-aware child failure, the Intersection
//! arm-drop + 0/1/many collapse, the Object empty-vs-surface split, the typed
//! surface-member carrier-arg fallbacks, the `Alias`/`TypeParam` cycle guards, the
//! fail-closed [`RaisedShapeAlgebra::absorb_dropped`] re-absorption) lives
//! ONCE here; the algebra trait + the interned term stay in the parent.

use std::sync::Arc;

use rustc_hash::FxHashSet;
use verter_span::Span;
use verter_type_expr::{LiteralValue, PrimitiveName};

use super::super::ProjectSemanticDispatch;
use super::conversions::{mapped_modifier_for_optionality, mapped_modifier_for_readonly};
use super::semantic_primitive_to_primitive_name;
use super::RaisedShapeAlgebra;
use crate::project_semantic_dispatch::{node_data_for, walk};
use crate::semantic_query::{IndexKey, QueryError, SemanticNodeData, SemanticNodeId, SurfaceView};

/// A folded tuple element awaiting algebra construction.
pub(super) struct FoldedTupleElement<O> {
    pub(super) label: Option<String>,
    pub(super) ty: O,
    pub(super) optional: bool,
    pub(super) rest: bool,
}

/// A folded function shape awaiting algebra construction.
pub(super) struct FoldedFunction<O> {
    pub(super) parameters: Vec<FoldedFunctionParam<O>>,
    pub(super) return_type: Option<O>,
    pub(super) type_parameters: Vec<FoldedTypeParam<O>>,
    pub(super) signature_span: Option<Span>,
    pub(super) return_type_span: Option<Span>,
}

pub(super) struct FoldedFunctionParam<O> {
    pub(super) name: Option<Arc<str>>,
    pub(super) ty: O,
    pub(super) optional: bool,
    pub(super) rest: bool,
    pub(super) span: Option<Span>,
}

pub(super) struct FoldedTypeParam<O> {
    pub(super) name: Arc<str>,
    pub(super) constraint: Option<O>,
    pub(super) default: Option<O>,
}

// ===========================================================================
// The single shared fold.
// ===========================================================================

/// The SOLE exhaustive `SemanticNodeData` traversal: raise `node` one
/// structural level at a time, recursing children, applying every raiser
/// transform structurally, and building the algebra's `Out`. `None` when the
/// node — or a `?`-propagating required child — is unavailable / unraisable
/// from the live graph store.
///
/// Cycle protection via the per-call `active` visited set, guarded explicitly
/// ONLY at `Alias` + `TypeParam` (insert / early-return-sentinel / remove); the
/// `Object` arm uses a FRESH `active` per member. This control flow descends
/// from the historical `raise_node_to_type_expr_core_impl` — now presence-aware
/// (a present-but-unraisable child fails the whole composite) with typed
/// surface-member carrier-arg fallbacks — re-housed so the materialization and
/// the node-domain facts/key share ONE traversal.
pub(super) fn fold_node<A: RaisedShapeAlgebra>(
    alg: &mut A,
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    active: &mut FxHashSet<SemanticNodeId>,
) -> Option<A::Out> {
    let ctx = dispatch.ctx;
    let data = node_data_for(ctx, node)?;
    Some(match data.as_ref() {
        SemanticNodeData::Primitive(kind) => {
            alg.primitive(semantic_primitive_to_primitive_name(*kind))
        }
        SemanticNodeData::Literal(value) => alg.literal(value.clone()),
        SemanticNodeData::Alias(target) => {
            if !active.insert(node) {
                return Some(alg.opaque_sentinel(&QueryError::RaiseAliasCycle));
            }
            let result = fold_node(alg, dispatch, *target, active);
            active.remove(&node);
            return result;
        }
        SemanticNodeData::Union(members) => {
            // Presence-aware: a PRESENT-but-unraisable member fails the WHOLE
            // composite (never silently erased).
            let folded: Vec<A::Out> = members
                .iter()
                .map(|member| fold_node(alg, dispatch, *member, active))
                .collect::<Option<_>>()?;
            alg.union(folded)
        }
        SemanticNodeData::Intersection(members) => {
            // Presence-aware recurse (a PRESENT-but-unraisable arm fails the
            // WHOLE composite), then drop the typed `UnrepresentableSurface`
            // degradation arms and the empty-object arms (`{} & X ≡ X`), then
            // collapse: empty -> empty object, len==1 -> that arm, else
            // Intersection. The recurse is materialised into a Vec FIRST so the
            // arm-drop inspection (an immutable `alg` borrow) does not overlap
            // the recurse closure's unique `alg` borrow.
            let arms: Vec<A::Out> = members
                .iter()
                .map(|member| fold_node(alg, dispatch, *member, active))
                .collect::<Option<_>>()?;
            let (mut kept, dropped): (Vec<A::Out>, Vec<A::Out>) =
                arms.into_iter().partition(|arm| {
                    !(alg.is_object_surface_sentinel(arm) || alg.is_empty_object(arm))
                });
            let result = if kept.is_empty() {
                alg.empty_object()
            } else if kept.len() == 1 {
                kept.drain(..).next().unwrap()
            } else {
                alg.intersection(kept)
            };
            // Fail-closed: the dropped arms' degradation rides the surviving
            // result (the materialize algebra absorbs the sidecars; the
            // node-domain algebras carry none).
            alg.absorb_dropped(result, dropped)
        }
        SemanticNodeData::Array { element, readonly } => {
            let element = fold_node(alg, dispatch, *element, active)?;
            alg.array(element, *readonly)
        }
        SemanticNodeData::Tuple { elements, readonly } => {
            // Presence-aware: a PRESENT-but-unraisable element fails the
            // WHOLE tuple (never silently erased).
            let folded: Vec<FoldedTupleElement<A::Out>> = elements
                .iter()
                .map(|element| {
                    Some(FoldedTupleElement {
                        label: element
                            .label
                            .as_ref()
                            .map(|label| label.as_ref().to_string()),
                        ty: fold_node(alg, dispatch, element.value, active)?,
                        optional: element.optional,
                        rest: element.rest,
                    })
                })
                .collect::<Option<_>>()?;
            alg.tuple(folded, *readonly)
        }
        SemanticNodeData::Object(surface) => {
            if surface.members.is_empty()
                && surface.call_signatures.is_empty()
                && surface.construct_signatures.is_empty()
                && !surface.has_index_signature
            {
                alg.empty_object()
            } else {
                fold_surface_view(alg, dispatch, surface)
                    .unwrap_or_else(|| alg.opaque_sentinel(&QueryError::UnrepresentableSurface))
            }
        }
        SemanticNodeData::MergedDecl { contributors } => {
            let merged = walk::reduce_merged_decl_with_graph(dispatch.graph(), contributors);
            return fold_node(alg, dispatch, merged, active);
        }
        SemanticNodeData::Opaque(QueryError::DeclPlaceholder { name, .. }) => {
            alg.reference(Arc::clone(name), Vec::new())
        }
        SemanticNodeData::Conditional {
            check,
            extends,
            true_branch_ref,
            false_branch_ref,
            ..
        } => {
            let check = fold_node(alg, dispatch, *check, active)?;
            let extends = fold_node(alg, dispatch, *extends, active)?;
            let true_type = fold_node(alg, dispatch, *true_branch_ref, active)?;
            let false_type = fold_node(alg, dispatch, *false_branch_ref, active)?;
            alg.conditional(check, extends, true_type, false_type)
        }
        SemanticNodeData::TemplateLiteral {
            quasis,
            expressions,
        } => {
            let quasis: Vec<String> = quasis
                .iter()
                .map(|quasi| quasi.as_ref().to_string())
                .collect();
            // Presence-aware: a PRESENT-but-unraisable expression fails the
            // WHOLE template literal (never silently erased).
            let expressions: Vec<A::Out> = expressions
                .iter()
                .map(|expr| fold_node(alg, dispatch, *expr, active))
                .collect::<Option<_>>()?;
            alg.template_literal(quasis, expressions)
        }
        SemanticNodeData::KeyOf { base } => {
            let base = fold_node(alg, dispatch, *base, active)?;
            alg.key_of(base)
        }
        SemanticNodeData::IndexedAccess { object, index } => {
            let object = fold_node(alg, dispatch, *object, active)?;
            let index = fold_index_key(alg, dispatch, index, active)?;
            alg.indexed_access(object, index)
        }
        SemanticNodeData::Mapped { mapper, .. } => {
            let parameter = match node_data_for(ctx, mapper.parameter_node).as_deref() {
                Some(SemanticNodeData::TypeParam { display_name, .. }) => {
                    display_name.as_ref().to_string()
                }
                _ => String::new(),
            };
            // The source recurses KeyOf-aware (matching the materializer's
            // explicit KeyOf shell around the mapped source key-space base).
            let source = match node_data_for(ctx, mapper.key_space)?.as_ref() {
                SemanticNodeData::KeyOf { base } => {
                    let base = fold_node(alg, dispatch, *base, active)?;
                    alg.key_of(base)
                }
                _ => fold_node(alg, dispatch, mapper.key_space, active)?,
            };
            let value = fold_node(alg, dispatch, mapper.value_expr, active)?;
            let optional = mapped_modifier_for_optionality(mapper.optionality);
            let readonly = mapped_modifier_for_readonly(mapper.readonly);
            let name_type = match mapper.name_remap {
                Some(remap) => Some(fold_node(alg, dispatch, remap, active)?),
                None => None,
            };
            alg.mapped(parameter, source, value, optional, readonly, name_type)
        }
        SemanticNodeData::TypeOf(_) => {
            let (value_root, path) = data.typeof_head().expect("TypeOf carrier head");
            let type_args = data.carrier_type_args();
            let mut segments = value_root
                .name
                .split('.')
                .map(|segment| segment.to_string())
                .collect::<Vec<_>>();
            segments.extend(path.iter().map(|segment| segment.as_ref().to_string()));
            let raised_args: Vec<A::Out> = type_args
                .iter()
                .map(|id| {
                    fold_node(alg, dispatch, *id, active).unwrap_or_else(|| {
                        alg.opaque_sentinel(&QueryError::UnrepresentableSurfaceMember)
                    })
                })
                .collect();
            alg.type_of(segments, raised_args)
        }
        SemanticNodeData::TypeParam {
            display_name,
            constraint,
            default,
            ..
        } => {
            if !active.insert(node) {
                return Some(alg.opaque_sentinel(&QueryError::TypeParamCycle));
            }
            // Presence-aware optional slots: ABSENT stays `None`, but a
            // PRESENT-but-unraisable constraint/default fails the WHOLE node
            // (`None` is never silently substituted for `Some(unraisable)`).
            let constraint_out = fold_optional_slot(alg, dispatch, *constraint, active);
            let default_out = fold_optional_slot(alg, dispatch, *default, active);
            active.remove(&node);
            let constraint_out = constraint_out?;
            let default_out = default_out?;
            alg.type_parameter(Arc::clone(display_name), constraint_out, default_out)
        }
        SemanticNodeData::Infer { name } | SemanticNodeData::InferRef { name } => {
            alg.infer(Arc::clone(name))
        }
        SemanticNodeData::Opaque(err) => match err {
            QueryError::RecursiveRef { name } => alg.recursive_ref(Arc::clone(name)),
            // The input is a typed `QueryError`, not a raw carrier — route it
            // through the typed `opaque_sentinel` entry (BORROWED — no clone on
            // this hot traversal arm). The materialize algebra emits the
            // byte-identical terminal projection
            // (`UnknownValue::compatibility_projection(semantic_query_error_raw(err))`);
            // the node-domain algebras classify directly from the typed
            // variant — the ONLY classification surface.
            _ => alg.opaque_sentinel(err),
        },
        SemanticNodeData::Signature {
            kind,
            params,
            return_type,
            type_parameters,
            signature_span,
            return_type_span,
        } => {
            let folded = fold_function(
                alg,
                dispatch,
                params,
                *return_type,
                type_parameters,
                *signature_span,
                *return_type_span,
                active,
            )?;
            let function = alg.build_function(folded);
            match kind {
                crate::semantic_query::SignatureKind::Call => alg.function_to_out(function),
                crate::semantic_query::SignatureKind::Construct => alg.constructor_to_out(function),
            }
        }
        SemanticNodeData::DeclRef { identity } => {
            alg.reference(Arc::clone(&identity.decl_name), Vec::new())
        }
        SemanticNodeData::InstantiationRef { base, args } => {
            let raised_args: Vec<A::Out> = args
                .iter()
                .map(|id| {
                    fold_node(alg, dispatch, *id, active).unwrap_or_else(|| {
                        alg.opaque_sentinel(&QueryError::UnrepresentableSurfaceMember)
                    })
                })
                .collect();
            alg.reference(Arc::clone(&base.decl_name), raised_args)
        }
        SemanticNodeData::BareRef(_) => {
            let (name, _scope) = data.bare_ref_head().expect("BareRef carrier head");
            let type_args = data.carrier_type_args();
            let raised_args: Vec<A::Out> = type_args
                .iter()
                .map(|id| {
                    fold_node(alg, dispatch, *id, active).unwrap_or_else(|| {
                        alg.opaque_sentinel(&QueryError::UnrepresentableSurfaceMember)
                    })
                })
                .collect();
            alg.reference(Arc::clone(name), raised_args)
        }
        SemanticNodeData::ImportType(_) => {
            let (specifier, qualifier, typeof_query) =
                data.import_type_head().expect("ImportType carrier head");
            let type_args = data.carrier_type_args();
            let raised_args: Vec<A::Out> = type_args
                .iter()
                .map(|id| {
                    fold_node(alg, dispatch, *id, active).unwrap_or_else(|| {
                        alg.opaque_sentinel(&QueryError::UnrepresentableSurfaceMember)
                    })
                })
                .collect();
            alg.import_type(
                Arc::clone(specifier),
                Arc::clone(qualifier),
                typeof_query,
                raised_args,
            )
        }
        SemanticNodeData::RawFallback { value } => alg.unknown(value.clone()),
        SemanticNodeData::SyntheticBinding { id, value_node } => {
            alg.synthetic_slot_binding(Arc::new(id.to_carrier_key(*value_node)))
        }
    })
}

/// Raise an [`IndexKey`] used as an `IndexedAccess` index — string / number
/// literals construct directly; a `TypeNode` recurses through the core.
fn fold_index_key<A: RaisedShapeAlgebra>(
    alg: &mut A,
    dispatch: &ProjectSemanticDispatch<'_>,
    index: &IndexKey,
    active: &mut FxHashSet<SemanticNodeId>,
) -> Option<A::Out> {
    Some(match index {
        IndexKey::String(text) => alg.literal(LiteralValue::String(text.as_ref().to_string())),
        IndexKey::Number(number) => alg.literal(LiteralValue::Number(number.get() as f64)),
        IndexKey::TypeNode(node) => fold_node(alg, dispatch, *node, active)?,
    })
}

/// Fold a [`SemanticNodeData::Signature`] payload into a [`FoldedFunction`].
/// `None` when any parameter, the required return, or a present-but-unraisable
/// type-param slot fails (presence-aware whole-composite failure).
#[allow(clippy::too_many_arguments)]
fn fold_function<A: RaisedShapeAlgebra>(
    alg: &mut A,
    dispatch: &ProjectSemanticDispatch<'_>,
    params: &[crate::semantic_query::FunctionParam],
    return_type: SemanticNodeId,
    type_parameters: &[crate::semantic_query::TypeParamDecl],
    signature_span: Option<Span>,
    return_type_span: Option<Span>,
    active: &mut FxHashSet<SemanticNodeId>,
) -> Option<FoldedFunction<A::Out>> {
    // Presence-aware: an unraisable parameter, the REQUIRED return, or a
    // present-but-unraisable type-param slot fails the WHOLE function.
    let parameters: Vec<FoldedFunctionParam<A::Out>> = params
        .iter()
        .map(|p| {
            Some(FoldedFunctionParam {
                name: p.name.clone(),
                ty: fold_node(alg, dispatch, p.ty, active)?,
                optional: p.optional,
                rest: p.rest,
                span: p.span,
            })
        })
        .collect::<Option<_>>()?;
    let return_out = fold_node(alg, dispatch, return_type, active)?;
    let mut type_params: Vec<FoldedTypeParam<A::Out>> = Vec::new();
    for tp in type_parameters {
        type_params.push(FoldedTypeParam {
            name: Arc::clone(&tp.name),
            constraint: fold_optional_slot(alg, dispatch, tp.constraint, active)?,
            default: fold_optional_slot(alg, dispatch, tp.default, active)?,
        });
    }
    Some(FoldedFunction {
        parameters,
        return_type: Some(return_out),
        type_parameters: type_params,
        signature_span,
        return_type_span,
    })
}

/// Fold a genuinely-optional slot (`constraint` / `default`): `Some(None)`
/// when the slot is ABSENT, `Some(Some(out))` when it raises, `None` when it
/// is PRESENT but unraisable (whole-composite failure — never silently
/// substituted with `None`).
fn fold_optional_slot<A: RaisedShapeAlgebra>(
    alg: &mut A,
    dispatch: &ProjectSemanticDispatch<'_>,
    slot: Option<SemanticNodeId>,
    active: &mut FxHashSet<SemanticNodeId>,
) -> Option<Option<A::Out>> {
    match slot {
        Some(node) => fold_node(alg, dispatch, node, active).map(Some),
        None => Some(None),
    }
}

/// Reconstruct an Object from a [`SurfaceView`] — the non-empty `Object` arm.
/// Each member / signature value folds through the core with a FRESH cycle set
/// (matching the materializer's fresh-per-member `active`). A member whose
/// value misses becomes the `SEMANTIC_SURFACE_MEMBER` sentinel. Returns `None`
/// when the surface yields no representable members (the empty-`{}` case is
/// handled by the caller).
fn fold_surface_view<A: RaisedShapeAlgebra>(
    alg: &mut A,
    dispatch: &ProjectSemanticDispatch<'_>,
    surface: &SurfaceView,
) -> Option<A::Out> {
    // Fold a member VALUE through the core with a fresh cycle set; a miss
    // becomes the SEMANTIC_SURFACE_MEMBER sentinel (matching the materializer).
    fn fold_member<A: RaisedShapeAlgebra>(
        alg: &mut A,
        dispatch: &ProjectSemanticDispatch<'_>,
        node: SemanticNodeId,
    ) -> A::Out {
        let mut active = FxHashSet::default();
        fold_node(alg, dispatch, node, &mut active)
            .unwrap_or_else(|| alg.opaque_sentinel(&QueryError::UnrepresentableSurfaceMember))
    }

    // Single-call-signature fast path: a surface with no members, no construct
    // signatures, no index signature, and exactly one call signature IS that
    // call signature's value (not wrapped in an object).
    if surface.members.is_empty()
        && surface.construct_signatures.is_empty()
        && !surface.has_index_signature
        && surface.call_signatures.len() == 1
    {
        return Some(fold_member(alg, dispatch, surface.call_signatures[0]));
    }

    let mut members: Vec<A::Member> = Vec::new();
    for member in surface.members.iter() {
        let ty = fold_member(alg, dispatch, member.value);
        if member.is_method {
            if let Some(function) = alg.out_as_function(&ty) {
                members.push(alg.member_method(
                    member.name.as_ref().to_string(),
                    function,
                    member.optional,
                    member.visibility,
                    member.spans,
                ));
                continue;
            }
        }
        members.push(alg.member_property(
            member.name.as_ref().to_string(),
            ty,
            member.optional,
            member.readonly,
            member.visibility,
            member.spans,
        ));
    }

    // Signatures that do not raise to a Function are dropped from the
    // object — but their degradation is absorbed (fail-closed), never
    // silently complete.
    let mut dropped: Vec<A::Out> = Vec::new();
    for signature in surface.call_signatures.iter() {
        let raised = fold_member(alg, dispatch, *signature);
        if let Some(function) = alg.out_as_function(&raised) {
            members.push(alg.member_call_signature(function));
        } else {
            dropped.push(raised);
        }
    }

    for signature in surface.construct_signatures.iter() {
        let raised = fold_member(alg, dispatch, *signature);
        if let Some(function) = alg.out_as_constructor(&raised) {
            members.push(alg.member_construct_signature(function));
        } else if let Some(function) = alg.out_as_function(&raised) {
            // Legacy call-kind entry in a construct bucket (surface
            // position defines construct-ness) — still publishes.
            members.push(alg.member_construct_signature(function));
        } else {
            dropped.push(raised);
        }
    }

    for signature in surface.index_signatures.iter() {
        let key_type = fold_member(alg, dispatch, signature.key_type);
        let value_type = fold_member(alg, dispatch, signature.value_type);
        members.push(alg.member_index_signature(
            "key".to_string(),
            key_type,
            value_type,
            signature.readonly,
            signature.spans,
        ));
    }

    // The synthetic open-surface placeholder ONLY when the surface is genuinely
    // OPEN (`has_index_signature` set, no concrete signature carried). Typed
    // degradation: the sidecar carries `QueryError::OpenSurface`; the compat
    // tree keeps the legacy `projectedOpenSurface` spelling.
    if surface.has_index_signature && surface.index_signatures.is_empty() {
        let key_type = alg.primitive(PrimitiveName::String);
        let value_type = alg.opaque_sentinel(&QueryError::OpenSurface);
        members.push(alg.member_index_signature(
            "key".to_string(),
            key_type,
            value_type,
            false,
            verter_type_expr::IndexSignatureSpans::default(),
        ));
    }

    if members.is_empty() {
        return None;
    }
    let out = alg.object_from_members(members);
    Some(alg.absorb_dropped(out, dropped))
}
