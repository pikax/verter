use std::borrow::Cow;

use verter_semantic::analysis::types::{
    AnalyzedImport, AnalyzedMacro, AnalyzedMacroKind, MacroTypeDep,
};
use verter_type_expr::TypeExpr;

use crate::resolver_core::ResolvedTypeDeclaration;

use super::ComponentMetaResolutionPurpose;

pub(super) fn should_ignore_external_macro_type(dep: &MacroTypeDep) -> bool {
    dep.macro_kind == AnalyzedMacroKind::DefineSlots
        && dep.import_source == "vue"
        && dep.type_name == "Slot"
}

pub(super) fn is_direct_macro_type_reference(
    macros: &[AnalyzedMacro],
    dep: &MacroTypeDep,
    _owner_source: Option<&str>,
) -> bool {
    let Some(mac) = macros.get(dep.macro_index) else {
        return false;
    };
    if !mac
        .type_references
        .iter()
        .any(|type_name| type_name == &dep.type_name)
    {
        return false;
    }

    // graph-native gate. The parsed type argument (cached
    // once during shallow analysis per the Shallow File Processing
    // Core Invariant) is the authoritative shape of the macro's first
    // type arg. When present, the dep is "direct" only if the parsed
    // expression carries a top-level (non-Object-property) reference
    // to `dep.type_name` reachable through Ref / Array / Tuple /
    // Intersection / Union / Conditional etc. — never through Object
    // members, which encode "nested" deps.
    //
    // When `parsed_type_argument` is `None` (the macro has no type
    // arguments OR shallow parsing failed), fall back to the
    // `mac.type_references` membership we already proved above
    // (`unwrap_or(true)` semantics preserved).
    mac.parsed_type_argument
        .as_deref()
        .map(|expr| type_expr_has_direct_macro_reference(expr, dep.type_name.as_str()))
        .unwrap_or(true)
}

/// Whether to keep a resolved imported macro entry for the `Full` path even
/// when the owner already has a projectable local surface.
///
/// This carves out the `defineProps<ImportedVueProps>()` case: the imported
/// component's surface is the authoritative one and must be kept in
/// `resolved_macros`, otherwise the owner's local projection would replace it.
pub(super) fn keep_direct_imported_vue_macro(
    projectable_owner_local: bool,
    purpose: ComponentMetaResolutionPurpose,
    macros: &[AnalyzedMacro],
    dep: &MacroTypeDep,
    owner_source: Option<&str>,
    declaration: &ResolvedTypeDeclaration,
) -> bool {
    projectable_owner_local
        && purpose == ComponentMetaResolutionPurpose::Full
        && is_direct_macro_type_reference(macros, dep, owner_source)
        && dep.macro_kind == AnalyzedMacroKind::DefineProps
        && declaration.canonical_source.ends_with(".vue")
}

// / clippy cleanup — `direct_macro_type_reference_expr`,
// `find_matching_angle`, and `split_top_level_type_args` were
// orphans (no caller in the landed tree). They originated as the legacy
// span-extraction path for cross-file `defineProps<T>()` macros before
// the dispatch-backed resolver took over. Removed in the
// clippy cleanup; the dispatch path is the sole canonical resolution.

fn type_expr_has_direct_macro_reference(expr: &TypeExpr, needle: &str) -> bool {
    match expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            name.as_ref() == needle
                || type_arguments
                    .iter()
                    .any(|arg| type_expr_has_direct_macro_reference(arg, needle))
        }
        TypeExpr::Intersection(types) | TypeExpr::Union(types) => types
            .iter()
            .any(|inner| type_expr_has_direct_macro_reference(inner, needle)),
        TypeExpr::Array { element, .. } => type_expr_has_direct_macro_reference(element, needle),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .any(|element| type_expr_has_direct_macro_reference(&element.ty, needle)),
        TypeExpr::Parenthesized(inner) | TypeExpr::Rest(inner) | TypeExpr::KeyOf(inner) => {
            type_expr_has_direct_macro_reference(inner, needle)
        }
        TypeExpr::TypeOf(value_ref) => value_ref.path.iter().any(|segment| segment == needle),
        TypeExpr::IndexedAccess { object, index } => {
            type_expr_has_direct_macro_reference(object, needle)
                || type_expr_has_direct_macro_reference(index, needle)
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            type_expr_has_direct_macro_reference(check, needle)
                || type_expr_has_direct_macro_reference(extends, needle)
                || type_expr_has_direct_macro_reference(true_type, needle)
                || type_expr_has_direct_macro_reference(false_type, needle)
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            type_expr_has_direct_macro_reference(source, needle)
                || type_expr_has_direct_macro_reference(value, needle)
                || name_type
                    .as_deref()
                    .is_some_and(|expr| type_expr_has_direct_macro_reference(expr, needle))
        }
        // A constructor type's signature is searched identically to a function
        // type's (same `FunctionExpr` payload).
        TypeExpr::Function(function) | TypeExpr::ConstructorType(function) => {
            function
                .parameters
                .iter()
                .any(|param| type_expr_has_direct_macro_reference(&param.ty, needle))
                || function
                    .return_type
                    .as_deref()
                    .is_some_and(|expr| type_expr_has_direct_macro_reference(expr, needle))
                || function.type_parameters.iter().any(|param| {
                    param
                        .constraint
                        .as_deref()
                        .is_some_and(|expr| type_expr_has_direct_macro_reference(expr, needle))
                        || param
                            .default
                            .as_deref()
                            .is_some_and(|expr| type_expr_has_direct_macro_reference(expr, needle))
                })
        }
        TypeExpr::TemplateLiteral { expressions, .. } => expressions
            .iter()
            .any(|expr| type_expr_has_direct_macro_reference(expr, needle)),
        TypeExpr::RecursiveRef {
            name,
            type_arguments,
            conditional_context,
        } => {
            name.as_ref() == needle
                || type_arguments
                    .iter()
                    .any(|arg| type_expr_has_direct_macro_reference(arg, needle))
                || conditional_context.iter().any(|ctx| {
                    type_expr_has_direct_macro_reference(&ctx.check, needle)
                        || type_expr_has_direct_macro_reference(&ctx.extends, needle)
                })
        }
        TypeExpr::TypeParameter(param) => param.name == needle,
        TypeExpr::Infer { name } => name == needle,
        TypeExpr::Object(_)
        | TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        // Synthetic carriers are intrinsic terminals; they reference no
        // workspace macro symbol.
        | TypeExpr::SyntheticSlotBinding(_)
        | TypeExpr::Unknown { .. } => false,
    }
}

pub(super) fn is_direct_local_macro_type_reference(
    mac: &AnalyzedMacro,
    resolved_index: usize,
    resolved_name: &str,
) -> bool {
    resolved_index == 0
        || mac
            .type_references
            .iter()
            .any(|type_name| type_name == resolved_name)
}

fn macro_has_authoritative_evaluated_surface(
    evaluated: Option<&verter_semantic::analysis::type_expand::ExpandedComponentTypes>,
    macro_kind: AnalyzedMacroKind,
    macro_index: usize,
) -> bool {
    let Some(evaluated) = evaluated else {
        return false;
    };

    match macro_kind {
        AnalyzedMacroKind::DefineProps
        | AnalyzedMacroKind::WithDefaults
        | AnalyzedMacroKind::DefineModel => evaluated
            .define_props
            .iter()
            .find(|entry| entry.macro_index == macro_index)
            .is_some_and(|entry| !entry.result.value.properties.is_empty()),
        AnalyzedMacroKind::DefineEmits => evaluated
            .define_emits
            .iter()
            .find(|entry| entry.macro_index == macro_index)
            .is_some_and(|entry| {
                !entry.result.value.properties.is_empty()
                    || !entry.result.value.call_signatures.is_empty()
            }),
        AnalyzedMacroKind::DefineSlots => evaluated
            .define_slots
            .iter()
            .find(|entry| entry.macro_index == macro_index)
            .is_some_and(|entry| !entry.result.value.properties.is_empty()),
        AnalyzedMacroKind::DefineExpose => !evaluated.bindings.is_empty(),
        AnalyzedMacroKind::DefineOptions => false,
    }
}

pub(super) fn macro_has_authoritative_owner_surface(
    mac: &AnalyzedMacro,
    evaluated: Option<&verter_semantic::analysis::type_expand::ExpandedComponentTypes>,
    macro_index: usize,
) -> bool {
    if macro_has_authoritative_evaluated_surface(evaluated, mac.kind, macro_index) {
        return true;
    }

    match mac.kind {
        AnalyzedMacroKind::DefineProps
        | AnalyzedMacroKind::WithDefaults
        | AnalyzedMacroKind::DefineModel => !mac.prop_fields.is_empty(),
        AnalyzedMacroKind::DefineEmits => !mac.emit_fields.is_empty(),
        AnalyzedMacroKind::DefineSlots => !mac.slot_fields.is_empty(),
        AnalyzedMacroKind::DefineExpose => !mac.expose_fields.is_empty(),
        AnalyzedMacroKind::DefineOptions => false,
    }
}

pub(super) fn macro_has_direct_local_type_root(mac: &AnalyzedMacro) -> bool {
    mac.resolved_local_types
        .iter()
        .enumerate()
        .any(|(resolved_index, resolved)| {
            is_direct_local_macro_type_reference(mac, resolved_index, resolved.name.as_str())
        })
}

pub(super) fn macro_dep_exported_type_name<'a>(
    imports: &'a [AnalyzedImport],
    dep: &'a MacroTypeDep,
) -> Cow<'a, str> {
    for import in imports
        .iter()
        .filter(|import| import.source == dep.import_source)
    {
        for binding in &import.bindings {
            if dep.type_name == binding.name {
                return Cow::Owned(
                    binding
                        .imported_name
                        .clone()
                        .unwrap_or_else(|| binding.name.clone()),
                );
            }

            if matches!(
                binding.kind,
                verter_semantic::analysis::types::ImportBindingKind::Namespace
            ) {
                let prefix = format!("{}.", binding.name);
                if let Some(member_name) = dep.type_name.strip_prefix(&prefix) {
                    return Cow::Owned(member_name.to_string());
                }
            }
        }
    }

    Cow::Borrowed(dep.type_name.as_str())
}

/// Whether an imported macro-type root should be seeded directly into the
/// initial registry on the cold-resolver path.
///
/// Under the graph-only / typed-IR resolver contract, declaration text is
/// not consumed for structural classification — the typed body lives on the
/// prepared decl and is read by downstream stages on demand. Non-TypeAlias
/// kinds (Interface / Class / Unknown) seed the registry directly so the
/// initial publication carries the graph-typed surface; TypeAlias kinds
/// also seed under the same contract because the only pre-cutover branch
/// that suppressed seeding (a non-Object alias body) was a text-driven
/// inspection that has been retired.
pub(super) fn should_seed_direct_macro_registry_entry(
    declaration: &ResolvedTypeDeclaration,
) -> bool {
    let _ = declaration;
    true
}

/// Whether an imported declaration's surface (as discovered through the
/// host's macro-elements path) is structurally authoritative — i.e. the
/// caller may publish the imported surface as the canonical projection
/// without re-routing through the structural resolver pipeline.
///
/// Under the graph-only / typed-IR resolver contract this answer is always
/// "no": the cold resolver does not have the typed body in scope at the
/// classification site, so the conservative answer is to defer to the
/// structural pipeline. The function is retained as a named pivot so
/// downstream callers (cold resolver, registry-seed-can-skip-refresh,
/// macro-shape materialiser) keep a single classification entry-point.
pub(crate) fn imported_declaration_surface_is_authoritative(
    declaration: &ResolvedTypeDeclaration,
) -> bool {
    let _ = declaration;
    false
}

pub(crate) fn imported_registry_seed_can_skip_refresh(
    owner_canonical: &str,
    declaration: &ResolvedTypeDeclaration,
    existing_expr: &TypeExpr,
) -> bool {
    !declaration.canonical_source.is_empty()
        && declaration.canonical_source != owner_canonical
        && imported_declaration_surface_is_authoritative(declaration)
        && crate::resolver_core::component_meta_registry::component_meta_registry_has_explicit_object_surface(existing_expr)
        && !crate::resolver_core::component_meta_registry::component_meta_registry_has_non_object_top_level_surface(existing_expr)
}
