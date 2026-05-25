use oxc_allocator::Allocator;
use verter_compiler::utils::oxc::vue::resolve_type::{
    ResolvedElements, ResolvedEmitSignature, ResolvedMemberVisibility,
};
use verter_semantic::analysis::jsdoc::extract_jsdoc_near_offset;
use verter_semantic::analysis::types::{
    AnalyzedEmitField, AnalyzedMacroKind, AnalyzedPropField, AnalyzedSlotField,
    AnalyzedSlotFieldBinding, JsdocTag,
};
use verter_type_expr::{
    FunctionExpr, FunctionParam, ObjectExpr, ObjectMember, ObjectProperty, TypeExpr, TypeExprScope,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedNativeProp {
    pub name: String,
    pub is_optional: bool,
    pub type_annotation: Option<String>,
    pub visibility: ResolvedMemberVisibility,
    pub span: verter_span::Span,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectedMacroSurfaces {
    pub native_props: Vec<ResolvedNativeProp>,
    pub props: Vec<AnalyzedPropField>,
    pub emits: Vec<AnalyzedEmitField>,
    pub slots: Vec<AnalyzedSlotField>,
    /// Lowered typed form of the entire `defineProps` / `withDefaults` /
    /// `defineModel` macro surface. Authoritative for downstream consumers
    /// (`projected_macro_surfaces_to_type_expr`, `cold_resolver`,
    /// `eval_program::project_imported_macro_surfaces`).
    pub props_expr: Option<TypeExpr>,
    /// Scope of `props_expr`: canonical_id of the file whose OXC parse
    /// produced the typed expression. Pairing invariant:
    /// `props_expr.is_some() <=> props_expr_scope.is_some()`.
    pub props_expr_scope: Option<TypeExprScope>,
    /// Lowered typed form of the entire `defineEmits` macro surface.
    pub emits_expr: Option<TypeExpr>,
    /// Scope of `emits_expr`. Pairing invariant:
    /// `emits_expr.is_some() <=> emits_expr_scope.is_some()`.
    pub emits_expr_scope: Option<TypeExprScope>,
    /// Lowered typed form of the entire `defineSlots` macro surface.
    pub slots_expr: Option<TypeExpr>,
    /// Scope of `slots_expr`. Pairing invariant:
    /// `slots_expr.is_some() <=> slots_expr_scope.is_some()`.
    pub slots_expr_scope: Option<TypeExprScope>,
}

pub fn project_macro_surfaces(
    source: Option<&str>,
    macro_kind: AnalyzedMacroKind,
    elements: &ResolvedElements,
) -> ProjectedMacroSurfaces {
    project_macro_surfaces_with_owner(source, None, macro_kind, elements)
}

/// Projector entry-point that propagates the local SFC's canonical_id so the
/// aggregate `ProjectedMacroSurfaces.*_expr_scope` fields can be stamped.
///
/// Per-field scopes (`AnalyzedPropField.type_expr_scope`,
/// `AnalyzedEmitField.payload_expr_scope`, `AnalyzedSlotField.return_expr_scope`)
/// are bridged from the parser-side `ResolvedProp`/`ResolvedEmit` regardless of
/// `owner_canonical` — they carry the file the OXC parse was performed in
/// (local SFC for inferred props, external file for cross-file resolved props).
///
/// `owner_canonical` is only used for the aggregate scope (the synthesized
/// `Object` covers the whole macro surface, so its scope is the SFC where the
/// macro was written). Pass `None` when the caller does not have the local
/// SFC's canonical_id; aggregate `*_expr` are then left as `None` because the
/// pairing invariant `*_expr.is_some() <=> *_expr_scope.is_some()` requires a
/// scope.
pub fn project_macro_surfaces_with_owner(
    source: Option<&str>,
    owner_canonical: Option<&str>,
    macro_kind: AnalyzedMacroKind,
    elements: &ResolvedElements,
) -> ProjectedMacroSurfaces {
    let native_props = collect_native_props(elements);

    match macro_kind {
        AnalyzedMacroKind::DefineProps
        | AnalyzedMacroKind::WithDefaults
        | AnalyzedMacroKind::DefineModel => {
            let props: Vec<AnalyzedPropField> = elements
                .props
                .iter()
                .filter(|prop| prop.visibility.is_public())
                .map(|prop| {
                    let (description, tags) = member_jsdoc(source, prop.span);
                    let type_expr = prop.type_expr.clone();
                    let type_expr_scope = prop.type_expr_scope.clone();
                    debug_assert_eq!(
                        type_expr.is_some(),
                        type_expr_scope.is_some(),
                        "AnalyzedPropField type_expr/type_expr_scope pairing violated for prop `{}`",
                        prop.key_name.as_deref().unwrap_or("<anon>")
                    );
                    AnalyzedPropField {
                        name: prop
                            .key_name
                            .clone()
                            .unwrap_or_else(|| "unknown".to_string()),
                        is_optional: prop.optional,
                        span: verter_span::Span::default(),
                        type_annotation: raw_prop_type_text(source, prop),
                        description,
                        tags,
                        resolution_source: verter_semantic::analysis::TypeResolutionSource::Rust,
                        resolution_error: None,
                        type_expr,
                        type_expr_scope,
                        declared_in_macro_type_arg: false,
                    }
                })
                .collect();

            let (props_expr, props_expr_scope) =
                build_aggregate_props_expr(&props, owner_canonical);

            ProjectedMacroSurfaces {
                native_props,
                props,
                emits: Vec::new(),
                slots: Vec::new(),
                props_expr,
                props_expr_scope,
                ..Default::default()
            }
        }
        AnalyzedMacroKind::DefineEmits => {
            let mut emits: Vec<AnalyzedEmitField> = elements
                .emits
                .iter()
                .map(|emit| {
                    let (description, tags) = member_jsdoc(source, emit.span);
                    let payload_type = raw_emit_payload_text(source, emit);
                    let payload_expr = emit.type_expr.clone();
                    let payload_expr_scope = emit.type_expr_scope.clone();
                    debug_assert_eq!(
                        payload_expr.is_some(),
                        payload_expr_scope.is_some(),
                        "AnalyzedEmitField payload_expr/payload_expr_scope pairing violated for emit `{}`",
                        emit.name
                    );
                    AnalyzedEmitField {
                        name: emit.name.clone(),
                        span: verter_span::Span::default(),
                        payload_type,
                        description,
                        tags,
                        payload_expr,
                        payload_expr_scope,
                    }
                })
                .collect();

            // Property-style emit definitions (e.g., from mapped types like
            // `{ [K in 'open' | 'close']?: [] }`) end up in `elements.props`
            // rather than `elements.emits`.  Convert them to emit entries when
            // no call-signature emits were found.
            if emits.is_empty() {
                let mut seen = rustc_hash::FxHashSet::default();
                for prop in &elements.props {
                    if !prop.visibility.is_public() {
                        continue;
                    }
                    let name = match &prop.key_name {
                        Some(name) => name.clone(),
                        None => continue,
                    };
                    if !seen.insert(name.clone()) {
                        continue;
                    }
                    let (description, tags) = member_jsdoc(source, prop.span);
                    let payload_type = raw_prop_type_text(source, prop);
                    let payload_expr = prop.type_expr.clone();
                    let payload_expr_scope = prop.type_expr_scope.clone();
                    debug_assert_eq!(
                        payload_expr.is_some(),
                        payload_expr_scope.is_some(),
                        "AnalyzedEmitField (property-style) payload_expr/payload_expr_scope pairing violated for emit `{}`",
                        name
                    );
                    emits.push(AnalyzedEmitField {
                        name,
                        span: verter_span::Span::default(),
                        payload_type,
                        description,
                        tags,
                        payload_expr,
                        payload_expr_scope,
                    });
                }
            }

            let (emits_expr, emits_expr_scope) =
                build_aggregate_emits_expr(&emits, owner_canonical);

            ProjectedMacroSurfaces {
                native_props,
                props: Vec::new(),
                emits,
                slots: Vec::new(),
                emits_expr,
                emits_expr_scope,
                ..Default::default()
            }
        }
        AnalyzedMacroKind::DefineSlots => {
            let slots: Vec<AnalyzedSlotField> = elements
                .props
                .iter()
                .filter(|prop| prop.visibility.is_public())
                .filter_map(|prop| {
                    let name = prop
                        .key_name
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string());
                    let (description, tags) = member_jsdoc(source, prop.span);
                    // Slot info is derived from the prop's typed function
                    // form. No source slicing, no `raw_type_text` reparse.
                    // Missing `type_expr` → no bindings and no return type
                    // (the `resolved_as_slot` flag still gates slot
                    // emission).
                    let (mut bindings, return_type) = prop
                        .type_expr
                        .as_ref()
                        .map(slot_info_from_type_expr)
                        .unwrap_or_else(|| (Vec::new(), None));
                    let resolved_as_slot = prop.types.iter().any(|runtime| {
                        matches!(
                            runtime,
                            verter_compiler::utils::oxc::vue::resolve_type::RuntimeType::Function
                        )
                    });
                    if bindings.is_empty() && return_type.is_none() && !resolved_as_slot {
                        return None;
                    }
                    // The slot prop's `type_expr` is the function type
                    // `(props: T) => R`. Pull the return type for `return_expr`
                    // and keep the same scope (the file whose OXC parse
                    // produced the slot signature). Non-function shapes leave
                    // `return_expr: None` (consumers fall back to display
                    // `return_type`).
                    let return_expr = prop
                        .type_expr
                        .as_ref()
                        .and_then(slot_return_expr_from_function_type);
                    let return_expr_scope =
                        return_expr.as_ref().and(prop.type_expr_scope.clone());
                    debug_assert_eq!(
                        return_expr.is_some(),
                        return_expr_scope.is_some(),
                        "AnalyzedSlotField return_expr/return_expr_scope pairing violated for slot `{}`",
                        name
                    );
                    // Stamp each binding's `binding_expr_scope` from the slot
                    // prop's own `type_expr_scope` — the function signature,
                    // its first param, and the bindings all live in the file
                    // where the slot prop's typed form was produced.
                    // `slot_info_from_type_expr` populates `binding_expr` but
                    // leaves `binding_expr_scope` as `None` (the walker has
                    // no scope information).
                    stamp_binding_scopes(prop.type_expr_scope.as_ref(), &mut bindings);
                    Some(AnalyzedSlotField {
                        name,
                        is_required: !prop.optional,
                        span: verter_span::Span::default(),
                        bindings,
                        return_type,
                        description,
                        tags,
                        return_expr,
                        return_expr_scope,
                    })
                })
                .collect();

            let (slots_expr, slots_expr_scope) =
                build_aggregate_slots_expr(&slots, owner_canonical);

            ProjectedMacroSurfaces {
                native_props,
                props: Vec::new(),
                emits: Vec::new(),
                slots,
                slots_expr,
                slots_expr_scope,
                ..Default::default()
            }
        }
        _ => ProjectedMacroSurfaces {
            native_props,
            props: Vec::new(),
            emits: Vec::new(),
            slots: Vec::new(),
            ..Default::default()
        },
    }
}

/// Pull the return type out of a `TypeExpr::Function` so a slot's `return_expr`
/// can be populated from the slot prop's typed function annotation. Returns
/// `None` for non-function shapes (where the slot's return type cannot be
/// recovered from the slot prop's type without consulting alias bodies, which
/// is a downstream resolver concern, not the projector's).
///
/// Iterative parenthesis unwrap: `((...((T) => R)))` resolves to `Some(R)`
/// without recursion. Bounded by `MAX_PAREN_UNWRAP` to satisfy the
/// resolver-core no-unbounded-recursion guard.
pub(crate) fn slot_return_expr_from_function_type(prop_type: &TypeExpr) -> Option<TypeExpr> {
    const MAX_PAREN_UNWRAP: usize = 32;
    let mut current = prop_type;
    for _ in 0..MAX_PAREN_UNWRAP {
        match current {
            TypeExpr::Function(function) => {
                return function.return_type.as_ref().map(|rt| (**rt).clone());
            }
            TypeExpr::Parenthesized(inner) => current = inner.as_ref(),
            _ => return None,
        }
    }
    None
}

/// Stamp the slot prop's `type_expr_scope` onto every binding that carries a
/// populated `binding_expr`. `slot_info_from_type_expr` produces bindings with
/// `binding_expr: Some(...)` but `binding_expr_scope: None` — the walker has
/// no scope information. The slot prop's own `type_expr_scope` is the file
/// where the function signature was parsed, which is also where each binding's
/// typed value originated.
///
/// Pairing invariant:
/// `binding_expr.is_some() <=> binding_expr_scope.is_some()` — if the slot
/// prop's scope is missing the binding's `binding_expr` is cleared so the
/// pair stays valid (rather than emitting an unscoped expr).
fn stamp_binding_scopes(
    prop_type_expr_scope: Option<&TypeExprScope>,
    bindings: &mut [AnalyzedSlotFieldBinding],
) {
    for binding in bindings.iter_mut() {
        if binding.binding_expr.is_none() {
            debug_assert!(
                binding.binding_expr_scope.is_none(),
                "AnalyzedSlotFieldBinding binding_expr/binding_expr_scope pairing violated (None expr with Some scope) for binding `{}`",
                binding.name
            );
            continue;
        }
        match prop_type_expr_scope {
            Some(scope) => binding.binding_expr_scope = Some(scope.clone()),
            None => {
                // No scope to stamp — drop the `binding_expr` so the pairing
                // invariant stays satisfied. Display `type_annotation` is
                // unaffected.
                binding.binding_expr = None;
                binding.binding_expr_scope = None;
            }
        }
        debug_assert_eq!(
            binding.binding_expr.is_some(),
            binding.binding_expr_scope.is_some(),
            "AnalyzedSlotFieldBinding binding_expr/binding_expr_scope pairing violated post-stamp for binding `{}`",
            binding.name
        );
    }
}

/// Synthesise the aggregate `props_expr` from the per-field typed forms.
///
/// Returns `(Some(Object), Some(scope))` only when every prop has a populated
/// `type_expr` AND `owner_canonical` is provided. Pairing invariant:
/// `props_expr.is_some() <=> props_expr_scope.is_some()`.
fn build_aggregate_props_expr(
    props: &[AnalyzedPropField],
    owner_canonical: Option<&str>,
) -> (Option<TypeExpr>, Option<TypeExprScope>) {
    let scope = match owner_canonical {
        Some(canonical) if !canonical.is_empty() => canonical,
        _ => return (None, None),
    };
    if props.is_empty() {
        return (None, None);
    }
    let mut properties: Vec<ObjectMember> = Vec::with_capacity(props.len());
    for prop in props {
        let ty = match &prop.type_expr {
            Some(ty) => ty.clone(),
            None => return (None, None),
        };
        properties.push(ObjectMember::Property(ObjectProperty {
            name: prop.name.clone(),
            ty,
            optional: prop.is_optional,
            readonly: false,
        }));
    }
    let aggregate = TypeExpr::Object(std::sync::Arc::new(ObjectExpr { properties }));
    let aggregate_scope = TypeExprScope::new(scope);
    debug_assert!(
        // Tautology after construction; pinning the invariant for readers.
        Some(&aggregate).is_some() && Some(&aggregate_scope).is_some(),
        "props_expr/props_expr_scope pairing violated"
    );
    (Some(aggregate), Some(aggregate_scope))
}

/// Synthesise the aggregate `emits_expr` from the per-field typed payloads.
///
/// Mirrors the shape `projected_macro_surfaces_to_type_expr` constructs from
/// raw text for the `DefineEmits` branch: a `TypeExpr::Object` whose properties
/// are `event_name: payload_expr`. Returns `(Some, Some)` only when every emit
/// has a populated `payload_expr` AND `owner_canonical` is provided.
fn build_aggregate_emits_expr(
    emits: &[AnalyzedEmitField],
    owner_canonical: Option<&str>,
) -> (Option<TypeExpr>, Option<TypeExprScope>) {
    let scope = match owner_canonical {
        Some(canonical) if !canonical.is_empty() => canonical,
        _ => return (None, None),
    };
    if emits.is_empty() {
        return (None, None);
    }
    let mut properties: Vec<ObjectMember> = Vec::with_capacity(emits.len());
    for emit in emits {
        let ty = match &emit.payload_expr {
            Some(ty) => ty.clone(),
            None => return (None, None),
        };
        properties.push(ObjectMember::Property(ObjectProperty {
            name: emit.name.clone(),
            ty,
            optional: false,
            readonly: false,
        }));
    }
    let aggregate = TypeExpr::Object(std::sync::Arc::new(ObjectExpr { properties }));
    (Some(aggregate), Some(TypeExprScope::new(scope)))
}

/// Synthesise the aggregate `slots_expr` from the per-field typed return types
/// and bindings.
///
/// Each slot becomes a property whose value is a function type
/// `(props: <bindings as Object>) => return_expr`. The bindings object is
/// constructed from the per-binding `binding_expr` values (typically populated
/// by `extract_slot_bindings_from_pick_ast_text`). Returns `(Some, Some)` only
/// when every slot has a populated `return_expr` AND every binding (if any)
/// has a populated `binding_expr` AND `owner_canonical` is provided.
fn build_aggregate_slots_expr(
    slots: &[AnalyzedSlotField],
    owner_canonical: Option<&str>,
) -> (Option<TypeExpr>, Option<TypeExprScope>) {
    let scope = match owner_canonical {
        Some(canonical) if !canonical.is_empty() => canonical,
        _ => return (None, None),
    };
    if slots.is_empty() {
        return (None, None);
    }
    let mut properties: Vec<ObjectMember> = Vec::with_capacity(slots.len());
    for slot in slots {
        let return_ty = match &slot.return_expr {
            Some(ty) => ty.clone(),
            None => return (None, None),
        };

        let binding_props: Vec<ObjectMember> = if slot.bindings.is_empty() {
            Vec::new()
        } else {
            let mut acc: Vec<ObjectMember> = Vec::with_capacity(slot.bindings.len());
            for binding in &slot.bindings {
                let ty = match &binding.binding_expr {
                    Some(ty) => ty.clone(),
                    None => return (None, None),
                };
                acc.push(ObjectMember::Property(ObjectProperty {
                    name: binding.name.clone(),
                    ty,
                    optional: false,
                    readonly: false,
                }));
            }
            acc
        };

        let mut parameters: Vec<FunctionParam> = Vec::new();
        if !binding_props.is_empty() {
            parameters.push(FunctionParam {
                name: Some("props".to_string()),
                ty: TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
                    properties: binding_props,
                })),
                optional: false,
                rest: false,
            });
        }
        let function = TypeExpr::Function(std::sync::Arc::new(FunctionExpr {
            parameters,
            return_type: Some(std::sync::Arc::new(return_ty)),
            type_parameters: Vec::new(),
        }));

        properties.push(ObjectMember::Property(ObjectProperty {
            name: slot.name.clone(),
            ty: function,
            optional: !slot.is_required,
            readonly: false,
        }));
    }
    let aggregate = TypeExpr::Object(std::sync::Arc::new(ObjectExpr { properties }));
    (Some(aggregate), Some(TypeExprScope::new(scope)))
}

/// When a type is not locally defined in a source file (e.g., barrel re-export),
/// find its import source specifiers and imported name so the caller can follow
/// the import chain. Handles `import { T } from '...'; export { T };`,
/// `export { T } from '...'`, and `export * from '...'` patterns.
///
/// Returns all candidates. For direct/named re-exports returns one entry.
/// For `export *` wildcards returns one entry per wildcard source.
pub fn find_type_import_sources_in_source(source: &str, type_name: &str) -> Vec<(String, String)> {
    use verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_program;

    let source = source.trim();
    if source.is_empty() {
        return Vec::new();
    }

    let alloc = Allocator::new();
    let source_type = oxc_span::SourceType::ts();
    let parsed = oxc_parser::Parser::new(&alloc, source, source_type).parse();
    if parsed.panicked {
        return Vec::new();
    }

    let analysis = analyze_external_type_program(&parsed.program);

    // Check direct re-export: `export { T } from '...'`
    if let Some((specifier, imported_name)) = analysis.direct_reexport_target(type_name) {
        return vec![(specifier.to_string(), imported_name.to_string())];
    }

    // Check import+export pattern: `import { T } from '...'; export { T };`
    let local_name = analysis
        .local_export_symbol_target(type_name)
        .unwrap_or(type_name);
    if let Some((specifier, imported_name)) = analysis.local_import_symbol_target(local_name) {
        return vec![(specifier.to_string(), imported_name.to_string())];
    }

    // Return all wildcard re-export sources as candidates
    analysis
        .wildcard_reexport_sources()
        .iter()
        .map(|specifier| (specifier.clone(), type_name.to_string()))
        .collect()
}

/// Given a source file and a locally-defined type name, return the imported
/// type dependencies that the type transitively references through its
/// heritage chain (extends, intersection, etc.).
///
/// Each entry is `(import_specifier, imported_name)` — e.g.,
/// `("vue-router", "RouterLinkProps")` for a type that extends an imported
/// interface.
pub fn find_heritage_type_imports_in_source(
    source: &str,
    type_name: &str,
) -> Vec<(String, String)> {
    use verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_program;

    let source = source.trim();
    if source.is_empty() {
        return Vec::new();
    }

    let alloc = Allocator::new();
    let source_type = oxc_span::SourceType::ts();
    let parsed = oxc_parser::Parser::new(&alloc, source, source_type).parse();
    if parsed.panicked {
        return Vec::new();
    }

    let analysis = analyze_external_type_program(&parsed.program);
    let required_import_names = analysis.required_import_names(type_name);

    required_import_names
        .into_iter()
        .filter_map(|name| {
            analysis
                .local_import_symbol_target(&name)
                .map(|(specifier, imported_name)| {
                    (specifier.to_string(), imported_name.to_string())
                })
        })
        .collect()
}

/// Extract slot binding fields and a display return-type from a slot prop's
/// typed function form `(props: T) => R`.
///
/// The caller supplies the slot prop's `type_expr` (lowered once during
/// shallow analysis); the walker reads typed IR only — no source slicing,
/// no text-mode reparse, no hand-rolled type-text splitters.
///
/// Walks the typed form:
/// - Iteratively unwraps `TypeExpr::Parenthesized` (bounded).
/// - For `TypeExpr::Function`, pulls the first parameter's `ty` and the
///   `return_type`.
/// - The first-param `ty` is walked to produce bindings:
///   * `TypeExpr::Object` — one binding per `ObjectMember::Property` with
///     `binding_expr` set to the property's typed value.
///   * `TypeExpr::Ref { name: "Pick", type_arguments: [obj, keys] }` —
///     one binding per key, with
///     `binding_expr = IndexedAccess { object: obj, index: key }`. Mirrors
///     the analyzer-side Pick walker contract. The displayed
///     `type_annotation` is the symbolic `Object[Key]` form so downstream
///     consumers can still see the requested member path.
///   * Anything else — no bindings emitted.
/// - The return-type display string is rendered from the typed return-type
///   via the inline `render_type_expr_display` helper (display-only).
///
/// Non-function shapes (e.g. plain object literals used as helper members)
/// return `(Vec::new(), None)`. The caller's `resolved_as_slot` flag still
/// decides whether to emit a slot at all.
///
/// The produced `AnalyzedSlotFieldBinding.binding_expr_scope` is `None` — the
/// walker does not know the slot prop's scope; the caller stamps the scope
/// from the slot prop's own `type_expr_scope` post-walk.
pub fn slot_info_from_type_expr(
    expr: &TypeExpr,
) -> (Vec<AnalyzedSlotFieldBinding>, Option<String>) {
    let Some(function) = unwrap_function_type(expr) else {
        return (Vec::new(), None);
    };

    let return_type = function
        .return_type
        .as_ref()
        .and_then(|rt| render_type_expr_display(rt));

    let Some(first_param) = function.parameters.first() else {
        return (Vec::new(), return_type);
    };

    let bindings = bindings_from_first_param_ty(&first_param.ty);
    (bindings, return_type)
}

/// Iteratively unwrap `TypeExpr::Parenthesized` and return the underlying
/// `FunctionExpr` if present. Bounded by `MAX_PAREN_UNWRAP` to satisfy the
/// resolver-core no-unbounded-recursion guard.
fn unwrap_function_type(expr: &TypeExpr) -> Option<&FunctionExpr> {
    const MAX_PAREN_UNWRAP: usize = 32;
    let mut current = expr;
    for _ in 0..MAX_PAREN_UNWRAP {
        match current {
            TypeExpr::Function(function) => return Some(function.as_ref()),
            TypeExpr::Parenthesized(inner) => current = inner.as_ref(),
            _ => return None,
        }
    }
    None
}

/// Walk the first-parameter `ty` of a slot's function type and emit one
/// `AnalyzedSlotFieldBinding` per binding key. See `slot_info_from_type_expr`
/// for the contract.
fn bindings_from_first_param_ty(ty: &TypeExpr) -> Vec<AnalyzedSlotFieldBinding> {
    const MAX_PAREN_UNWRAP: usize = 32;
    let mut current = ty;
    for _ in 0..MAX_PAREN_UNWRAP {
        match current {
            TypeExpr::Object(object) => {
                return bindings_from_object_expr(object.as_ref());
            }
            TypeExpr::Ref {
                name,
                type_arguments,
            } if name.as_ref() == "Pick" && type_arguments.len() == 2 => {
                return bindings_from_pick_args(&type_arguments[0], &type_arguments[1]);
            }
            TypeExpr::Parenthesized(inner) => current = inner.as_ref(),
            _ => return Vec::new(),
        }
    }
    Vec::new()
}

/// Emit one binding per `ObjectMember::Property` in `object`. Each binding's
/// `binding_expr` is the property's typed value. `type_annotation` renders the
/// typed value to a display string.
fn bindings_from_object_expr(object: &ObjectExpr) -> Vec<AnalyzedSlotFieldBinding> {
    object
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(property) => {
                let display = render_type_expr_display(&property.ty);
                Some(AnalyzedSlotFieldBinding {
                    name: property.name.clone(),
                    type_annotation: display,
                    span: verter_span::Span::default(),
                    binding_expr: Some(property.ty.clone()),
                    // Scope is filled in by the caller from the slot prop's
                    // own `type_expr_scope` — the walker does not know it.
                    binding_expr_scope: None,
                })
            }
            _ => None,
        })
        .collect()
}

/// Emit one binding per key in a `Pick<Object, Keys>` reference. The
/// `binding_expr` for each binding is
/// `TypeExpr::IndexedAccess { object, index }` so consumers can navigate
/// against the resolved `Object` without re-deriving the slot membership.
/// The displayed `type_annotation` is the symbolic `Object[Key]` form (e.g.
/// `CalendarCellTriggerProps['day']`).
fn bindings_from_pick_args(
    object_ty: &TypeExpr,
    keys_ty: &TypeExpr,
) -> Vec<AnalyzedSlotFieldBinding> {
    let object_arc = std::sync::Arc::new(object_ty.clone());
    let object_display = render_type_expr_display(object_ty);
    let mut bindings: Vec<AnalyzedSlotFieldBinding> = Vec::new();
    let mut push_for_key = |key_expr: &TypeExpr| {
        let (binding_name, key_display) = match key_expr {
            TypeExpr::Literal(verter_type_expr::LiteralValue::String(value)) => {
                (value.clone(), format!("'{value}'"))
            }
            TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty() => (name.to_string(), name.to_string()),
            _ => return,
        };
        let display = object_display
            .as_ref()
            .map(|obj| format!("{obj}[{key_display}]"));
        let index_arc = std::sync::Arc::new(key_expr.clone());
        let binding_expr = TypeExpr::IndexedAccess {
            object: object_arc.clone(),
            index: index_arc,
        };
        bindings.push(AnalyzedSlotFieldBinding {
            name: binding_name,
            type_annotation: display,
            span: verter_span::Span::default(),
            binding_expr: Some(binding_expr),
            binding_expr_scope: None,
        });
    };

    match keys_ty {
        TypeExpr::Union(arms) => {
            for arm in arms.iter() {
                push_for_key(arm);
            }
        }
        single => push_for_key(single),
    }
    bindings
}

/// Render a `TypeExpr` to a display string for `AnalyzedSlotFieldBinding`
/// and `AnalyzedSlotField.return_type`. Display-only; semantic decisions
/// must read the typed form. Returns `None` for shapes the renderer cannot
/// surface as a single inline display fragment.
///
/// Bounded by `MAX_DISPLAY_DEPTH` to satisfy the resolver-core
/// no-unbounded-recursion guard. The `depth_budget` decrements on each
/// nested call; when it hits zero the renderer returns `None` (the caller
/// surfaces `type_annotation: None`, and the typed `binding_expr` /
/// `return_expr` remains authoritative).
fn render_type_expr_display(expr: &TypeExpr) -> Option<String> {
    const MAX_DISPLAY_DEPTH: usize = 64;
    render_type_expr_display_inner(expr, MAX_DISPLAY_DEPTH)
}

fn render_type_expr_display_inner(expr: &TypeExpr, depth_budget: usize) -> Option<String> {
    use verter_type_expr::{LiteralValue, PrimitiveName};

    if depth_budget == 0 {
        return None;
    }
    let next = depth_budget - 1;

    match expr {
        TypeExpr::Primitive(name) => Some(match name {
            PrimitiveName::String => "string".to_string(),
            PrimitiveName::Number => "number".to_string(),
            PrimitiveName::Boolean => "boolean".to_string(),
            PrimitiveName::BigInt => "bigint".to_string(),
            PrimitiveName::Symbol => "symbol".to_string(),
            PrimitiveName::Null => "null".to_string(),
            PrimitiveName::Undefined => "undefined".to_string(),
            PrimitiveName::Void => "void".to_string(),
            PrimitiveName::Any => "any".to_string(),
            PrimitiveName::Unknown => "unknown".to_string(),
            PrimitiveName::Never => "never".to_string(),
            PrimitiveName::Object => "object".to_string(),
        }),
        // TS convention: string literals render with single quotes
        // (matching `'foo'` in indexed accesses like `Foo['bar']`). The
        // inner content is left as-is (the parser preserves the literal
        // content; embedded single quotes are not escaped because the
        // source TS would have used double quotes in that case).
        TypeExpr::Literal(LiteralValue::String(value)) => Some(format!("'{value}'")),
        TypeExpr::Literal(LiteralValue::Number(value)) => Some(value.to_string()),
        TypeExpr::Literal(LiteralValue::Boolean(value)) => Some(value.to_string()),
        TypeExpr::Literal(LiteralValue::BigInt(value)) => Some(value.clone()),
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            if type_arguments.is_empty() {
                Some(name.to_string())
            } else {
                let args: Option<Vec<String>> = type_arguments
                    .iter()
                    .map(|ty| render_type_expr_display_inner(ty, next))
                    .collect();
                Some(format!("{}<{}>", name, args?.join(", ")))
            }
        }
        TypeExpr::Union(types) => {
            let parts: Option<Vec<String>> = types
                .iter()
                .map(|ty| render_type_expr_display_inner(ty, next))
                .collect();
            Some(parts?.join(" | "))
        }
        TypeExpr::Intersection(types) => {
            let parts: Option<Vec<String>> = types
                .iter()
                .map(|ty| render_type_expr_display_inner(ty, next))
                .collect();
            Some(parts?.join(" & "))
        }
        TypeExpr::Array { element, readonly } => {
            let rendered = render_type_expr_display_inner(element, next)?;
            Some(if *readonly {
                format!("readonly {rendered}[]")
            } else {
                format!("{rendered}[]")
            })
        }
        TypeExpr::Tuple { elements, readonly } => {
            let parts: Option<Vec<String>> = elements
                .iter()
                .map(|element| {
                    let mut rendered = String::new();
                    if let Some(label) = &element.label {
                        rendered.push_str(label);
                        if element.optional {
                            rendered.push('?');
                        }
                        rendered.push_str(": ");
                    }
                    if element.rest {
                        rendered.push_str("...");
                    }
                    rendered.push_str(&render_type_expr_display_inner(&element.ty, next)?);
                    Some(rendered)
                })
                .collect();
            let joined = parts?.join(", ");
            Some(if *readonly {
                format!("readonly [{joined}]")
            } else {
                format!("[{joined}]")
            })
        }
        TypeExpr::IndexedAccess { object, index } => {
            let obj = render_type_expr_display_inner(object, next)?;
            let idx = render_type_expr_display_inner(index, next)?;
            Some(format!("{obj}[{idx}]"))
        }
        TypeExpr::Parenthesized(inner) => Some(format!(
            "({})",
            render_type_expr_display_inner(inner, next)?
        )),
        _ => None,
    }
}

fn collect_native_props(elements: &ResolvedElements) -> Vec<ResolvedNativeProp> {
    elements
        .props
        .iter()
        .map(|prop| ResolvedNativeProp {
            name: prop
                .key_name
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
            is_optional: prop.optional,
            type_annotation: raw_prop_type_text(None, prop),
            visibility: prop.visibility,
            span: verter_span::Span::new(prop.span.start, prop.span.end),
        })
        .collect()
}

fn raw_prop_type_text(
    source: Option<&str>,
    prop: &verter_compiler::utils::oxc::vue::resolve_type::ResolvedProp,
) -> Option<String> {
    prop.type_text.clone().or_else(|| {
        prop.type_span
            .and_then(|span| slice_source_span(source, span))
    })
}

fn raw_emit_payload_text(
    source: Option<&str>,
    emit: &verter_compiler::utils::oxc::vue::resolve_type::ResolvedEmit,
) -> Option<String> {
    // Display-only formatter. Prefer the raw source-span text (which
    // preserves any conditional / generic surface text the resolver may
    // have simplified), falling back to the pre-baked
    // `params_text` / `tuple_text` carried on `ResolvedEmit.signature`
    // when the source span is unavailable.
    slice_source_span(source, emit.span)
        .and_then(|text| raw_emit_payload_text_from_source(&text, &emit.signature))
        .or_else(|| match &emit.signature {
            ResolvedEmitSignature::Call { params_text } => {
                if params_text.is_empty() {
                    None
                } else {
                    Some(format!("[{}]", params_text))
                }
            }
            ResolvedEmitSignature::Tuple { tuple_text } => Some(tuple_text.clone()),
        })
}

/// Display-only formatter for an emit's payload text, given the raw source
/// span of the signature.
///
/// The Tuple branch returns the substring after the first top-level `:`
/// (the property key colon). The Call branch joins the post-name parameter
/// slices into a synthetic tuple display.
///
/// Walks the source bytes with a minimal state machine — tracks string
/// quoting and brace/paren/bracket/angle nesting.
///
/// **Typed-IR-Only Resolver Rule carve-out (display-only allowlist).**
///
/// This helper is the single sanctioned source-text-walking formatter
/// inside `surface_projector.rs`. Its purpose is preserving EXACT source
/// text for display (including whitespace, comments inside the
/// annotation, conditional/generic surface text the resolver may have
/// simplified upstream). A typed-form renderer would change the
/// formatting characteristics in subtle ways (whitespace
/// re-canonicalisation, comment stripping, structural normalisation of
/// `Function(params, return)` shapes), breaking the
/// `payload_type: Option<String>` display-passthrough contract the JS
/// compat layer relies on.
///
/// Allowlist conditions:
/// - Output flows ONLY to `AnalyzedEmitField.payload_type` (display-only)
///   and downstream `PropertyMeta.type` / `EventMeta.rawSignature`
///   passthroughs. NO consumer in the resolver / projector / registry /
///   policy / materialiser pipeline parses this output back.
/// - Semantic decisions live on the typed `AnalyzedEmitField.payload_expr`
///   sidecar (populated by the parser-side W0.3 producer chain).
///
/// `nesting_aware_split` is a private nested helper scoped to this
/// function. It does NOT participate in semantic resolution and MUST NOT
/// be hoisted to a sibling helper module that would expose it to
/// resolver-pipeline consumers.
fn raw_emit_payload_text_from_source(
    signature_text: &str,
    signature: &ResolvedEmitSignature,
) -> Option<String> {
    fn nesting_aware_split(text: &str, separator: char, first_only: bool) -> Vec<&str> {
        let mut parts = Vec::new();
        let mut start = 0usize;
        let mut paren = 0i32;
        let mut bracket = 0i32;
        let mut brace = 0i32;
        let mut angle = 0i32;
        let mut in_string = false;
        let mut string_delim = '\0';
        let mut escape = false;
        for (idx, ch) in text.char_indices() {
            if in_string {
                if escape {
                    escape = false;
                    continue;
                }
                if ch == '\\' {
                    escape = true;
                    continue;
                }
                if ch == string_delim {
                    in_string = false;
                }
                continue;
            }
            match ch {
                '\'' | '"' | '`' => {
                    in_string = true;
                    string_delim = ch;
                }
                '(' => paren += 1,
                ')' => paren -= 1,
                '[' => bracket += 1,
                ']' => bracket -= 1,
                '{' => brace += 1,
                '}' => brace -= 1,
                '<' => angle += 1,
                '>' => angle -= 1,
                _ if ch == separator && paren == 0 && bracket == 0 && brace == 0 && angle == 0 => {
                    parts.push(&text[start..idx]);
                    start = idx + ch.len_utf8();
                    if first_only {
                        return parts;
                    }
                }
                _ => {}
            }
        }
        parts.push(&text[start..]);
        parts
    }
    match signature {
        ResolvedEmitSignature::Tuple { .. } => {
            // First top-level colon separates the property key from the value type.
            let parts = nesting_aware_split(signature_text, ':', true);
            // After the colon, the remainder of the signature_text is the value.
            // `nesting_aware_split` with `first_only` puts only the prefix in `parts`;
            // the tail is the substring after the colon.
            let prefix = parts.first()?;
            let tail_start = prefix.len() + ':'.len_utf8();
            if tail_start > signature_text.len() {
                return None;
            }
            let tail = signature_text[tail_start..]
                .trim()
                .trim_end_matches([';', ','])
                .trim();
            (!tail.is_empty()).then(|| tail.to_string())
        }
        ResolvedEmitSignature::Call { .. } => {
            let open = signature_text.find('(')?;
            // Find the matching close-paren accounting for nesting.
            let bytes = signature_text.as_bytes();
            let mut depth = 0i32;
            let mut close = None;
            let mut in_str = false;
            let mut delim = b' ';
            let mut esc = false;
            for (i, &b) in bytes.iter().enumerate().skip(open) {
                if in_str {
                    if esc {
                        esc = false;
                        continue;
                    }
                    if b == b'\\' {
                        esc = true;
                        continue;
                    }
                    if b == delim {
                        in_str = false;
                    }
                    continue;
                }
                match b {
                    b'\'' | b'"' | b'`' => {
                        in_str = true;
                        delim = b;
                    }
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            close = Some(i);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let close = close?;
            let inner = &signature_text[open + 1..close];
            let params = nesting_aware_split(inner, ',', false);
            let payload_params: Vec<_> = params
                .into_iter()
                .skip(1)
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect();
            Some(format!("[{}]", payload_params.join(", ")))
        }
    }
}

fn slice_source_span(source: Option<&str>, span: verter_span::Span) -> Option<String> {
    let source = source?;
    let start = span.start as usize;
    let end = span.end as usize;
    if start >= end || end > source.len() {
        return None;
    }
    let text = source[start..end]
        .trim()
        .trim_end_matches([';', ','])
        .trim()
        .to_string();
    (!text.is_empty()).then_some(text)
}

fn member_jsdoc(source: Option<&str>, span: verter_span::Span) -> (Option<String>, Vec<JsdocTag>) {
    let Some(source) = source else {
        return (None, Vec::new());
    };
    extract_jsdoc_near_offset(source, span.start)
}
