//! Umbrella native-side gate for residual operator leaves.
//!
//! After `raise_and_reduce` runs, the resolved `TypeExpr` payload MUST NOT
//! contain `IndexedAccess` / `Conditional` / `Mapped` / `KeyOf` / `TypeOf` /
//! `TemplateLiteral` / `Infer` / `Rest` operator leaves at any nested
//! position EXCEPT under three documented exceptions:
//!
//! - (a) The operator is replaced by `TypeExpr::Unknown { raw }` —
//!   represented by the absence of the operator from the tree, since `Unknown`
//!   does not carry nested `TypeExpr`.
//! - (b) Inside the `type_arguments` / `conditional_context` of a
//!   `TypeExpr::RecursiveRef` (the recursive cycle preserves operator forms
//!   so consumers can detect the cycle structurally).
//! - (c) **Open-deferred form** — the operator's operands transitively
//!   contain a free `TypeExpr::TypeParameter` (semantically meaningful
//!   symbolic forms; eliminating them would lose information per
//!   CLAUDE.md "Macro Type Traversal Rule").
//!
//! This is the NATIVE-side gate, not a post-bridge string check: the
//! resolved payload must carry zero residual operator leaves outside
//! the three exceptions.
//!
//! The fixtures are `Avatar.vue`'s `size` prop and a
//! `Pick<HelperProps, ...>`-style macro fixture mirroring the corpus's
//! most common offender.

use std::sync::Arc;

use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

use verter_semantic::analysis::component_meta::ComponentMetaAnalysis;
use verter_type_expr::{ObjectMember, TypeExpr};

const FIXTURE_HELPER: &str = "export interface HelperProps {\n\
    size?: 'sm' | 'md' | 'lg'\n\
    name?: string\n\
    description?: string\n\
}\n";

const FIXTURE_AVATAR: &str = "<script setup lang=\"ts\">\n\
defineProps<{\n\
  size?: 'sm' | 'md' | 'lg'\n\
  src?: string\n\
}>()\n\
</script>\n\
<template><div /></template>\n";

const FIXTURE_CARD: &str = "<script setup lang=\"ts\">\n\
import type { HelperProps } from './Helper'\n\
defineProps<Pick<HelperProps, 'size' | 'name'>>()\n\
</script>\n\
<template><div /></template>\n";

fn host_with_fixtures() -> Arc<VerterHost> {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/Helper.ts".into()),
        input_id: "/Helper.ts".into(),
        source: Arc::from(FIXTURE_HELPER),
        file_language: FileLanguage::script_ts(),
        aliases: vec![],
    });
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/Avatar.vue".into()),
        input_id: "/Avatar.vue".into(),
        source: Arc::from(FIXTURE_AVATAR),
        file_language: FileLanguage::vue(),
        aliases: vec![],
    });
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/Card.vue".into()),
        input_id: "/Card.vue".into(),
        source: Arc::from(FIXTURE_CARD),
        file_language: FileLanguage::vue(),
        aliases: vec![],
    });
    host.set_import_dependencies(
        "/Card.vue",
        vec![verter_session::DependencyResolution {
            specifier: "./Helper".to_string(),
            resolved_canonical_id: Some("/Helper.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host
}

/// Returns true if the type expression transitively contains a free
/// `TypeParameter` (open-deferred form, exception class (c)).
fn contains_free_type_parameter(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::TypeParameter(_) => true,
        TypeExpr::Primitive(_) | TypeExpr::Literal(_) => false,
        TypeExpr::Unknown { .. } | TypeExpr::Infer { .. } => false,
        // Synthetic carriers carry no embedded type parameter — closed
        // intrinsic identity.
        TypeExpr::SyntheticSlotBinding(_) => false,
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            types.iter().any(contains_free_type_parameter)
        }
        TypeExpr::Array { element, .. } => contains_free_type_parameter(element),
        TypeExpr::Tuple { elements, .. } => {
            elements.iter().any(|e| contains_free_type_parameter(&e.ty))
        }
        TypeExpr::Object(obj) => obj.properties.iter().any(|m| match m {
            ObjectMember::Property(prop) => contains_free_type_parameter(&prop.ty),
            ObjectMember::Method(method) => {
                method
                    .function
                    .parameters
                    .iter()
                    .any(|p| contains_free_type_parameter(&p.ty))
                    || method
                        .function
                        .return_type
                        .as_deref()
                        .map(contains_free_type_parameter)
                        .unwrap_or(false)
            }
            ObjectMember::CallSignature(f) | ObjectMember::ConstructSignature(f) => {
                f.parameters
                    .iter()
                    .any(|p| contains_free_type_parameter(&p.ty))
                    || f.return_type
                        .as_deref()
                        .map(contains_free_type_parameter)
                        .unwrap_or(false)
            }
            ObjectMember::IndexSignature(idx) => contains_free_type_parameter(&idx.value_type),
        }),
        // A constructor type's signature is searched identically to a function
        // type's (same `FunctionExpr` payload).
        TypeExpr::Function(func) | TypeExpr::ConstructorType(func) => {
            func.parameters
                .iter()
                .any(|p| contains_free_type_parameter(&p.ty))
                || func
                    .return_type
                    .as_deref()
                    .map(contains_free_type_parameter)
                    .unwrap_or(false)
        }
        TypeExpr::Ref { type_arguments, .. } => {
            type_arguments.iter().any(contains_free_type_parameter)
        }
        TypeExpr::KeyOf(inner) | TypeExpr::Rest(inner) | TypeExpr::Parenthesized(inner) => {
            contains_free_type_parameter(inner)
        }
        TypeExpr::TypeOf(_) => false,
        TypeExpr::IndexedAccess { object, index } => {
            contains_free_type_parameter(object) || contains_free_type_parameter(index)
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            contains_free_type_parameter(check)
                || contains_free_type_parameter(extends)
                || contains_free_type_parameter(true_type)
                || contains_free_type_parameter(false_type)
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            contains_free_type_parameter(source)
                || contains_free_type_parameter(value)
                || name_type
                    .as_ref()
                    .map(|n| contains_free_type_parameter(n))
                    .unwrap_or(false)
        }
        TypeExpr::TemplateLiteral { expressions, .. } => {
            expressions.iter().any(contains_free_type_parameter)
        }
        TypeExpr::RecursiveRef { type_arguments, .. } => {
            type_arguments.iter().any(contains_free_type_parameter)
        }
        // An import-type site carries free type parameters only through its
        // applied type arguments (`import("m").Generic<T>`); the specifier and
        // qualifier are plain strings.
        TypeExpr::ImportType { type_arguments, .. } => {
            type_arguments.iter().any(contains_free_type_parameter)
        }
    }
}

/// Walk the resolved payload and collect any operator-leaf violations.
/// Returns one human-readable violation string per offending position,
/// each prefixed with `prop_name :: path`.
///
/// The walker honors the three documented exception classes:
/// - (a) `Unknown { raw }` is a sink (no recursion).
/// - (b) `RecursiveRef.type_arguments` / `conditional_context` are NOT
///   recursed into (the recursive context is allowed to retain operator
///   forms — they encode the cycle).
/// - (c) Operators whose operands transitively contain a free
///   `TypeParameter` are skipped (open-deferred form).
fn find_residual_operator_leaves(
    host: &verter_session::VerterHost,
    owner: &str,
    meta: &ComponentMetaAnalysis,
) -> Vec<String> {
    // Demand-materialize each published source through the ONE shared
    // dispatch (the raise -> reduce step whose output this gate walks).
    // A payload-less source (`None`) carries no operator leaves.
    let demand = |source: &verter_type_expr::facts::SemanticTypeSource, what: &str| {
        verter_session::test_only::semantic_source_probe::demand_type_expr(host, owner, source)
            .unwrap_or_else(|| panic!("{what}'s published source must demand-materialize"))
    };
    let mut violations = Vec::new();
    for prop in &meta.props {
        if let Some(source) = prop.type_source.present() {
            let ty = demand(source, &format!("prop:{}", prop.name));
            walk(&ty, &format!("prop:{}", prop.name), &mut violations);
        }
    }
    for event in &meta.events {
        if let Some(source) = event.payload.present() {
            let ty = demand(source, &format!("event:{}", event.name));
            walk(&ty, &format!("event:{}", event.name), &mut violations);
        }
    }
    for slot in &meta.slots {
        for binding in &slot.bindings {
            if let Some(source) = binding.type_source.present() {
                let ty = demand(source, &format!("slot:{}.{}", slot.name, binding.name));
                walk(
                    &ty,
                    &format!("slot:{}.{}", slot.name, binding.name),
                    &mut violations,
                );
            }
        }
    }
    violations
}

fn walk(expr: &TypeExpr, path: &str, out: &mut Vec<String>) {
    let bad_kind = match expr {
        TypeExpr::IndexedAccess { .. } => Some("IndexedAccess"),
        TypeExpr::Conditional { .. } => Some("Conditional"),
        TypeExpr::Mapped { .. } => Some("Mapped"),
        TypeExpr::KeyOf(_) => Some("KeyOf"),
        TypeExpr::TypeOf(_) => Some("TypeOf"),
        TypeExpr::TemplateLiteral { .. } => Some("TemplateLiteral"),
        TypeExpr::Infer { .. } => Some("Infer"),
        TypeExpr::Rest(_) => Some("Rest"),
        _ => None,
    };
    if let Some(kind) = bad_kind {
        // Exception class (c): if the operator is open-deferred over a
        // free TypeParameter, it is allowed.
        if !contains_free_type_parameter(expr) {
            out.push(format!("{path} :: {kind} :: {expr:?}"));
        }
        // Whether allowed or not, do NOT descend into the operator's
        // operands further — counting the outer operator is enough.
        return;
    }

    match expr {
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::TypeParameter(_)
        // Synthetic carriers are closed terminal leaves with no
        // operator descendants.
        | TypeExpr::SyntheticSlotBinding(_)
        | TypeExpr::Unknown { .. } => {}
        // Exception class (b): RecursiveRef's payload is allowed to
        // retain operator forms (they encode the cycle).
        TypeExpr::RecursiveRef { .. } => {}
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            for (i, ty) in types.iter().enumerate() {
                walk(ty, &format!("{path}[{i}]"), out);
            }
        }
        TypeExpr::Array { element, .. } => {
            walk(element, &format!("{path}[]"), out);
        }
        TypeExpr::Tuple { elements, .. } => {
            for (i, elem) in elements.iter().enumerate() {
                walk(&elem.ty, &format!("{path}[tup{i}]"), out);
            }
        }
        TypeExpr::Object(obj) => {
            for (i, m) in obj.properties.iter().enumerate() {
                match m {
                    ObjectMember::Property(prop) => {
                        walk(&prop.ty, &format!("{path}.{}", prop.name), out);
                    }
                    ObjectMember::Method(method) => {
                        for (j, p) in method.function.parameters.iter().enumerate() {
                            walk(&p.ty, &format!("{path}.{}.param{j}", method.name), out);
                        }
                        if let Some(ret) = method.function.return_type.as_deref() {
                            walk(ret, &format!("{path}.{}.return", method.name), out);
                        }
                    }
                    ObjectMember::CallSignature(f) | ObjectMember::ConstructSignature(f) => {
                        for (j, p) in f.parameters.iter().enumerate() {
                            walk(&p.ty, &format!("{path}.call{i}.param{j}"), out);
                        }
                        if let Some(ret) = f.return_type.as_deref() {
                            walk(ret, &format!("{path}.call{i}.return"), out);
                        }
                    }
                    ObjectMember::IndexSignature(idx) => {
                        walk(&idx.value_type, &format!("{path}.[index{i}]"), out);
                    }
                }
            }
        }
        // A constructor type's signature is walked identically to a function
        // type's (same `FunctionExpr` payload).
        TypeExpr::Function(func) | TypeExpr::ConstructorType(func) => {
            for (i, p) in func.parameters.iter().enumerate() {
                walk(&p.ty, &format!("{path}.param{i}"), out);
            }
            if let Some(ret) = func.return_type.as_deref() {
                walk(ret, &format!("{path}.return"), out);
            }
        }
        TypeExpr::Ref { type_arguments, .. } => {
            for (i, ty) in type_arguments.iter().enumerate() {
                walk(ty, &format!("{path}<arg{i}>"), out);
            }
        }
        // An import-type site is walked like a `Ref`: its applied type
        // arguments are the only operator-bearing descendants (the specifier
        // and qualifier are plain strings).
        TypeExpr::ImportType { type_arguments, .. } => {
            for (i, ty) in type_arguments.iter().enumerate() {
                walk(ty, &format!("{path}<import-arg{i}>"), out);
            }
        }
        TypeExpr::Parenthesized(inner) => {
            walk(inner, path, out);
        }
        TypeExpr::IndexedAccess { .. }
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::KeyOf(_)
        | TypeExpr::TypeOf(_)
        | TypeExpr::TemplateLiteral { .. }
        | TypeExpr::Infer { .. }
        | TypeExpr::Rest(_) => {
            // Already handled above.
        }
    }
}

#[test]
fn avatar_concrete_inline_props_have_no_residual_operator_leaves() {
    // Avatar.vue uses an inline object literal — every prop should
    // resolve to a concrete shape. There are no type parameters in
    // scope, so exception class (c) does not apply. Any residual
    // operator leaf indicates `raise_and_reduce` failed to reduce a
    // concrete operator.
    let host = host_with_fixtures();
    let meta = host
        .get_component_meta("/Avatar.vue")
        .expect("Avatar.vue must produce component meta");

    let violations = find_residual_operator_leaves(&host, "/Avatar.vue", &meta);
    assert!(
        violations.is_empty(),
        "umbrella gate (Avatar.vue): resolved type_expr \
         payload contains residual operator leaves outside the three \
         documented exceptions. raise_and_reduce should reduce concrete \
         operators or convert hard-stops to Unknown {{ raw }}. \
         Violations:\n  {}",
        violations.join("\n  "),
    );
}

#[test]
fn card_pick_utility_has_no_residual_operator_leaves() {
    // Card.vue uses `Pick<HelperProps, 'size' | 'name'>` — a concrete
    // utility application. After raise_and_reduce, the result should
    // be a concrete object shape, no IndexedAccess / Conditional / etc.
    // residual leaves.
    let host = host_with_fixtures();
    let meta = host
        .get_component_meta("/Card.vue")
        .expect("Card.vue must produce component meta");

    let violations = find_residual_operator_leaves(&host, "/Card.vue", &meta);
    assert!(
        violations.is_empty(),
        "umbrella gate (Card.vue Pick<...>): resolved \
         type_expr payload contains residual operator leaves outside the \
         three documented exceptions. The `Pick<HelperProps, 'size' | \
         'name'>` macro should resolve to concrete object members. \
         Violations:\n  {}",
        violations.join("\n  "),
    );
}
