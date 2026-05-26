//! Published-surface field-type reducer + reducible-operator predicate.
//!
//! This module hosts the two
//! helpers the projector pipeline needs to finalise published field
//! shapes — the per-pipeline driver that runs the shared field-type
//! reducer over every `evaluated_types` row, and the structural
//! predicate that decides whether the bounded fixed-point reducer
//! needs to run for a given `TypeExpr`.
//!
//! Historical note: the retired `field_reduce.rs` module hosted the
//! same two helpers plus a projector-side name-predicate carrier
//! check (`is_builtin_utility_instantiation`,
//! `generic_instantiation_body_is_object`). The codex-hybrid retires
//! the projector-side carrier check by routing carrier-stop through
//! the dispatch demand context — only the type-shape predicate and
//! the field-type reducer remain.

use verter_type_expr::TypeExpr;

use crate::semantic_query::ProjectionMode;

use super::{reduce_field_type_expr, reduce_field_type_expr_with_mode};

/// Run the shared field-type reducer over every published surface in
/// `evaluated_types` so consumers see the same finalised shapes the
/// per-macro projectors already publish for `props` / `emits`.
///
/// Published macro props /
/// emits are reduced under `ProjectionMode::Navigate`. The dispatch's
/// reduction-demand context propagates this through every nested
/// operator dispatch — `keyof T` and `{ [K in S]: V }` carrier-stop
/// when `may_reduce_operator` rejects the context. Explicit narrowing
/// (`IndexedAccess`, finite `Pick`/`Omit`, closed/open conditionals)
/// still reduces path-precisely because those callers enter the
/// reducer with a `Published + Expanded` context downstream.
///
/// Synthetic slot-binding carriers (the `slot_bindings` / `bindings`
/// loops) are SHORT-CIRCUITED when both:
///
/// * `evaluated_types.carrier_provenance_table.get(surface, &field.name)`
///   returns `Some(provenance)` — the producer recorded this as a
///   synthetic `Ref { name }` carrier minted by the slot-binding
///   graph publisher's no-parser-branch, AND
/// * the host's `CarrierVerdictDb` returns `Some(DoNotDeepen)` for
///   the carrier's identity (eagerly admitted by the producer at
///   carrier-mint time).
///
/// On a hit we leave `field.r#type` unchanged — the published carrier
/// flows downstream as-is. This skip is the structural cost driver
/// codex's R22 verdict identified: `reduce_field_type_expr` on a bare
/// symbolic `Ref { name }` re-enters the resolver through registry
/// collection (`resolver_core/component_meta_registry.rs:1806/1870/1947`
/// → `host_manage/component_meta_methods.rs:2384/2583`), which the
/// next commit's component-meta-registry refuse-to-enqueue closes at
/// the source. Skipping the reducer here is the consumer-side half
/// of that closure.
///
/// R22-fix sparse-sidecar variant: instead of consulting a per-field
/// `Option<CarrierProvenance>` on every published field, the table
/// lookup runs only on `slot_bindings` / `bindings` — props / emits
/// pay no carrier-aware cost.
pub(crate) fn reduce_published_field_types(
    scope_canonical_id: &str,
    evaluated_types: &mut verter_semantic::analysis::type_expand::ExpandedComponentTypes,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) {
    use crate::meta_resolve::compare_type_expr_improvement;
    use rustc_hash::FxHashMap;
    use verter_semantic::analysis::type_expand::PublishedSurfaceKind;

    let carrier_verdicts = query_engine.ctx.project_type_store().carrier_verdicts();

    let mut finalized_prop_types: FxHashMap<String, TypeExpr> = FxHashMap::default();
    for field in evaluated_types.props.iter_mut() {
        let raised = std::mem::replace(&mut field.r#type, TypeExpr::Unknown { raw: String::new() });
        let mut reduced = reduce_field_type_expr_with_mode(
            query_engine,
            scope_canonical_id,
            raised,
            ProjectionMode::Navigate,
        );

        if let Some(shallow) = field.shallow_type_expr.as_ref() {
            if !matches!(shallow, TypeExpr::Unknown { .. })
                && compare_type_expr_improvement(shallow, &reduced)
            {
                let shallow_reduced = reduce_field_type_expr_with_mode(
                    query_engine,
                    scope_canonical_id,
                    shallow.clone(),
                    ProjectionMode::Navigate,
                );
                if compare_type_expr_improvement(&shallow_reduced, &reduced) {
                    reduced = shallow_reduced;
                }
            }
        }

        finalized_prop_types.insert(field.name.clone(), reduced.clone());
        field.r#type = reduced;
    }
    for define_props in evaluated_types.define_props.iter_mut() {
        for property in define_props.result.value.properties.iter_mut() {
            if let Some(finalised) = finalized_prop_types.get(property.name.as_str()) {
                property.ty = finalised.clone();
            }
        }
    }
    for field in evaluated_types.emits.iter_mut() {
        let raised = std::mem::replace(&mut field.r#type, TypeExpr::Unknown { raw: String::new() });
        field.r#type = reduce_field_type_expr_with_mode(
            query_engine,
            scope_canonical_id,
            raised,
            ProjectionMode::Navigate,
        );
    }
    // Split-borrow: the carrier-aware loops below need a shared
    // borrow of `carrier_provenance_table` concurrent with the
    // mutable borrows of the sibling `slot_bindings` / `bindings`
    // Vecs. Destructure once so the borrow checker treats the
    // sub-borrows as disjoint.
    let verter_semantic::analysis::type_expand::ExpandedComponentTypes {
        ref carrier_provenance_table,
        ref mut slot_bindings,
        ref mut bindings,
        ..
    } = *evaluated_types;
    for field in slot_bindings.iter_mut() {
        if should_skip_carrier_reduction(
            PublishedSurfaceKind::SlotBinding,
            field,
            carrier_provenance_table,
            carrier_verdicts,
        ) {
            continue;
        }
        let raised = std::mem::replace(&mut field.r#type, TypeExpr::Unknown { raw: String::new() });
        field.r#type = reduce_field_type_expr(query_engine, scope_canonical_id, raised);
    }
    for field in bindings.iter_mut() {
        if should_skip_carrier_reduction(
            PublishedSurfaceKind::Binding,
            field,
            carrier_provenance_table,
            carrier_verdicts,
        ) {
            continue;
        }
        let raised = std::mem::replace(&mut field.r#type, TypeExpr::Unknown { raw: String::new() });
        field.r#type = reduce_field_type_expr(query_engine, scope_canonical_id, raised);
    }
}

/// Returns `true` when a published slot-binding / binding field's
/// `r#type` is a synthetic `TypeExpr::Ref { name }` carrier whose
/// `CarrierProvenance` (looked up in the parent
/// `ExpandedComponentTypes::carrier_provenance_table` by surface
/// kind + field name) matches a `DoNotDeepen` verdict in the
/// host-owned cache. Caller is `reduce_published_field_types`'s
/// slot_bindings / bindings loops.
///
/// The R22-fix sparse-sidecar variant: the table is consulted only
/// at this carrier-aware site; props/emits never call this helper
/// and therefore pay no lookup cost.
fn should_skip_carrier_reduction(
    surface: verter_semantic::analysis::type_expand::PublishedSurfaceKind,
    field: &verter_semantic::analysis::type_expand::ExpandedField,
    carrier_provenance_table: &verter_semantic::analysis::type_expand::CarrierProvenanceTable,
    carrier_verdicts: &crate::carrier_verdict_db::CarrierVerdictDb,
) -> bool {
    let Some(provenance) = carrier_provenance_table.get(surface, field.name.as_str()) else {
        return false;
    };
    // Belt-and-braces: provenance presence is the producer's marker
    // but we still verify the field's `r#type` is the bare
    // `Ref { name }` carrier shape codex's verdict identifies as the
    // structural cost driver. A parser-path-only entry has no table
    // record and is filtered above; a synthetic carrier whose
    // downstream re-typing produced something other than a bare
    // `Ref` (would be a defect) falls through to the reducer rather
    // than incorrectly short-circuiting.
    let is_bare_ref_carrier = matches!(
        &field.r#type,
        TypeExpr::Ref { name, type_arguments }
            if name.as_ref() == provenance.binding_name.as_ref() && type_arguments.is_empty()
    );
    if !is_bare_ref_carrier {
        return false;
    }
    let key = crate::carrier_verdict_db::CarrierIdentity::from_provenance(provenance);
    carrier_verdicts.is_do_not_deepen(&key)
}

/// Does `expr` contain any operator-shape node that the bounded
/// fixed-point reducer should resolve?
///
/// Returns `true` when the expression carries an `IndexedAccess`,
/// `KeyOf`, `TypeOf`, `Conditional`, `Mapped`, or `Infer` anywhere
/// in its tree. Used by `reduce_field_type_expr` to decide whether
/// to skip the reducer entirely.
pub(crate) fn type_expr_contains_reducible_operator(expr: &TypeExpr) -> bool {
    use verter_type_expr::ObjectMember;

    match expr {
        TypeExpr::IndexedAccess { .. }
        | TypeExpr::KeyOf(_)
        | TypeExpr::TypeOf(_)
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::Infer { .. } => true,
        TypeExpr::Parenthesized(inner) | TypeExpr::Rest(inner) => {
            type_expr_contains_reducible_operator(inner)
        }
        TypeExpr::Array { element, .. } => type_expr_contains_reducible_operator(element),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .any(|el| type_expr_contains_reducible_operator(&el.ty)),
        TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
            members.iter().any(type_expr_contains_reducible_operator)
        }
        TypeExpr::Object(object) => object.properties.iter().any(|m| match m {
            ObjectMember::Property(p) => type_expr_contains_reducible_operator(&p.ty),
            ObjectMember::Method(method) => {
                method
                    .function
                    .parameters
                    .iter()
                    .any(|param| type_expr_contains_reducible_operator(&param.ty))
                    || method
                        .function
                        .return_type
                        .as_deref()
                        .is_some_and(type_expr_contains_reducible_operator)
            }
            ObjectMember::IndexSignature(sig) => {
                type_expr_contains_reducible_operator(&sig.key_type)
                    || type_expr_contains_reducible_operator(&sig.value_type)
            }
            ObjectMember::CallSignature(f) | ObjectMember::ConstructSignature(f) => {
                f.parameters
                    .iter()
                    .any(|p| type_expr_contains_reducible_operator(&p.ty))
                    || f.return_type
                        .as_deref()
                        .is_some_and(type_expr_contains_reducible_operator)
            }
        }),
        TypeExpr::Function(f) => {
            f.parameters
                .iter()
                .any(|p| type_expr_contains_reducible_operator(&p.ty))
                || f.return_type
                    .as_deref()
                    .is_some_and(type_expr_contains_reducible_operator)
        }
        TypeExpr::Ref { type_arguments, .. } => type_arguments
            .iter()
            .any(type_expr_contains_reducible_operator),
        TypeExpr::TemplateLiteral { expressions, .. } => expressions
            .iter()
            .any(type_expr_contains_reducible_operator),
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::TypeParameter(_)
        | TypeExpr::RecursiveRef { .. }
        // Synthetic carriers carry no reducible operators — they are
        // intrinsic terminal leaves.
        | TypeExpr::SyntheticSlotBinding(_)
        | TypeExpr::Unknown { .. } => false,
    }
}
