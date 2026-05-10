use oxc_allocator::Allocator;
use oxc_span::GetSpan;
use verter_compiler::utils::oxc::vue::resolve_type::{
    resolve_external_type, ResolvedElements, ResolvedEmitSignature, ResolvedMemberVisibility,
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
                    let raw_type_text = raw_prop_type_text(source, prop);
                    let (bindings, return_type) =
                        extract_slot_info_from_type_text(source, raw_type_text.as_deref());
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
fn slot_return_expr_from_function_type(prop_type: &TypeExpr) -> Option<TypeExpr> {
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

pub fn extract_slot_info_from_type_text(
    source: Option<&str>,
    type_text: Option<&str>,
) -> (Vec<AnalyzedSlotFieldBinding>, Option<String>) {
    let Some(text) = type_text else {
        return (Vec::new(), None);
    };

    // Parse the slot signature once via OXC and walk the AST. The signature
    // shape we accept is `(props: T) => R` where T may be a `TSTypeLiteral`
    // (`{ row: MyItem }`) or a `TSTypeReference` (`Pick<X, K>` / a userland
    // alias). Bindings are recovered from T directly via the analyzer-side
    // typed walker (`extract_slot_bindings_from_pick_ast`) plus the synthetic
    // declaration fallback for non-Pick shapes.
    let wrapper = format!("type __SlotSig = {text};");
    let alloc_for_parse = Allocator::new();
    let parser = oxc_parser::Parser::new(&alloc_for_parse, &wrapper, oxc_span::SourceType::ts());
    let parsed = parser.parse();

    let mut signature_ast: Option<&oxc_ast::ast::TSType<'_>> = None;
    for stmt in &parsed.program.body {
        if let oxc_ast::ast::Statement::TSTypeAliasDeclaration(alias) = stmt {
            signature_ast = Some(&alias.type_annotation);
            break;
        }
    }

    let mut return_type: Option<String> = None;
    let mut binding_param_ast: Option<&oxc_ast::ast::TSType<'_>> = None;

    if let Some(oxc_ast::ast::TSType::TSFunctionType(fn_type)) = signature_ast {
        // Recover the return-type display from the wrapper source span.
        let rt_span = fn_type.return_type.type_annotation.span();
        let rt_start = rt_span.start as usize;
        let rt_end = rt_span.end as usize;
        if rt_end <= wrapper.len() {
            let rt_text = wrapper[rt_start..rt_end].trim();
            if !rt_text.is_empty() {
                return_type = Some(rt_text.to_string());
            }
        }
        // First-parameter type annotation drives binding extraction.
        if let Some(first_param) = fn_type.params.items.first() {
            if let Some(ta) = first_param.type_annotation.as_ref() {
                binding_param_ast = Some(&ta.type_annotation);
            }
        }
    }

    let Some(binding_ts) = binding_param_ast else {
        return (Vec::new(), return_type);
    };

    // Walk `Pick<Object, Keys>` directly — emit `IndexedAccess { object, index }`
    // for each binding. Falls through to the synthetic-declaration path for
    // non-Pick shapes (typeRefs, type literals, etc.).
    let pick_bindings = extract_slot_bindings_from_pick_ast_text(binding_ts, &wrapper);
    if !pick_bindings.is_empty() {
        return (pick_bindings, return_type);
    }

    // Fallback: produce a synthetic interface containing the binding type and
    // resolve via the existing parser path. The original source (when
    // available) is concatenated so the synthetic declaration can reference
    // local types.
    let binding_span = binding_ts.span();
    let binding_text = wrapper[binding_span.start as usize..binding_span.end as usize].trim();
    let binding_declaration = if binding_text.starts_with('{') {
        format!("export interface _Bindings {binding_text}")
    } else {
        format!("export type _Bindings = {binding_text}")
    };
    let synthetic = source
        .filter(|source| !source.trim().is_empty())
        .map(|source| format!("{source}\n{binding_declaration}"))
        .unwrap_or(binding_declaration);

    let alloc = Allocator::new();
    let Some(resolved) = resolve_external_type("_Bindings", &synthetic, &alloc) else {
        return (Vec::new(), return_type);
    };

    let bindings = resolved
        .props
        .iter()
        .filter_map(|prop| {
            let name = prop.key_name.as_ref()?.clone();
            let type_annotation = if binding_text.starts_with('{') {
                prop.type_text.clone()
            } else {
                Some(format!("{binding_text}['{name}']"))
            };
            Some(AnalyzedSlotFieldBinding {
                name,
                type_annotation,
                span: verter_span::Span::default(),
                binding_expr: None,
                binding_expr_scope: None,
            })
        })
        .collect();

    (bindings, return_type)
}

/// Recover slot bindings from an AST `Pick<Object, Keys>` type reference.
///
/// Mirrors the analyzer-side walker in
/// `verter_semantic::analysis::macros::extract_slot_bindings_from_pick_ast`.
/// Walks the OXC `TSType` directly — no source slicing for semantic decisions.
/// For each key in `args[1]`:
/// - String-literal keys emit
///   `binding_expr = TypeExpr::IndexedAccess { object: lower(args[0]), index: Literal(String(k)) }`.
/// - Userland alias keys (TSTypeReference) emit
///   `binding_expr = TypeExpr::IndexedAccess { object: lower(args[0]), index: Ref { name: "K" } }`.
///   Alias resolution is NOT analyzer scope — the projector / cross-file
///   resolver walks the `Ref` lazily.
fn extract_slot_bindings_from_pick_ast_text(
    ts_type: &oxc_ast::ast::TSType<'_>,
    source: &str,
) -> Vec<AnalyzedSlotFieldBinding> {
    use oxc_ast::ast::{TSLiteral, TSType, TSTypeName};

    let TSType::TSTypeReference(type_ref) = ts_type else {
        return Vec::new();
    };
    let is_pick = matches!(
        &type_ref.type_name,
        TSTypeName::IdentifierReference(id) if id.name == "Pick"
    );
    if !is_pick {
        return Vec::new();
    }
    let Some(type_args) = type_ref.type_arguments.as_ref() else {
        return Vec::new();
    };
    if type_args.params.len() != 2 {
        return Vec::new();
    }
    let object_ts = &type_args.params[0];
    let keys_ts = &type_args.params[1];

    let object_expr = std::sync::Arc::new(verter_type_expr_oxc::lower_ts_type(object_ts, source));
    let object_text = {
        let span = object_ts.span();
        let st = span.start as usize;
        let en = span.end as usize;
        if en <= source.len() {
            source[st..en].trim().to_string()
        } else {
            String::new()
        }
    };

    let mut bindings = Vec::new();
    let push_for_key =
        |key_ts: &TSType<'_>, bindings: &mut Vec<AnalyzedSlotFieldBinding>| match key_ts {
            TSType::TSLiteralType(lit) => {
                if let TSLiteral::StringLiteral(s) = &lit.literal {
                    let key_name = s.value.to_string();
                    let key_text = {
                        let span = lit.span();
                        let st = span.start as usize;
                        let en = span.end as usize;
                        if en <= source.len() {
                            source[st..en].trim().to_string()
                        } else {
                            format!("\"{key_name}\"")
                        }
                    };
                    let display =
                        (!object_text.is_empty()).then(|| format!("{object_text}[{key_text}]"));
                    let index_expr =
                        TypeExpr::Literal(verter_type_expr::LiteralValue::String(key_name.clone()));
                    let binding_expr = Some(TypeExpr::IndexedAccess {
                        object: object_expr.clone(),
                        index: std::sync::Arc::new(index_expr),
                    });
                    let binding_expr_scope = binding_expr.as_ref().map(|_| TypeExprScope::new(""));
                    bindings.push(AnalyzedSlotFieldBinding {
                        name: key_name,
                        type_annotation: display,
                        span: verter_span::Span::default(),
                        binding_expr,
                        binding_expr_scope,
                    });
                }
            }
            TSType::TSTypeReference(key_ref) => {
                let alias_name = match &key_ref.type_name {
                    TSTypeName::IdentifierReference(id) => Some(id.name.to_string()),
                    _ => None,
                };
                if let Some(alias_name) = alias_name {
                    let key_text = {
                        let span = key_ts.span();
                        let st = span.start as usize;
                        let en = span.end as usize;
                        if en <= source.len() {
                            source[st..en].trim().to_string()
                        } else {
                            alias_name.clone()
                        }
                    };
                    let display =
                        (!object_text.is_empty()).then(|| format!("{object_text}[{key_text}]"));
                    let index_expr = verter_type_expr_oxc::lower_ts_type(key_ts, source);
                    let binding_expr = Some(TypeExpr::IndexedAccess {
                        object: object_expr.clone(),
                        index: std::sync::Arc::new(index_expr),
                    });
                    let binding_expr_scope = binding_expr.as_ref().map(|_| TypeExprScope::new(""));
                    bindings.push(AnalyzedSlotFieldBinding {
                        name: alias_name,
                        type_annotation: display,
                        span: verter_span::Span::default(),
                        binding_expr,
                        binding_expr_scope,
                    });
                }
            }
            _ => {}
        };

    match keys_ts {
        TSType::TSUnionType(union) => {
            for arm in &union.types {
                push_for_key(arm, &mut bindings);
            }
        }
        single => push_for_key(single, &mut bindings),
    }

    bindings
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
/// quoting and brace/paren/bracket/angle nesting. This is a display
/// formatter only; semantic decisions live in the typed
/// `*_expr` consumers.
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
