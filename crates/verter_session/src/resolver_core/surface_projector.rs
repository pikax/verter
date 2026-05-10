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
use verter_type_expr::{TypeExpr, TypeExprScope};

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
    let native_props = collect_native_props(elements);

    match macro_kind {
        AnalyzedMacroKind::DefineProps
        | AnalyzedMacroKind::WithDefaults
        | AnalyzedMacroKind::DefineModel => {
            let props = elements
                .props
                .iter()
                .filter(|prop| prop.visibility.is_public())
                .map(|prop| {
                    let (description, tags) = member_jsdoc(source, prop.span);
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
                        type_expr: None,
                        type_expr_scope: None,
                    }
                })
                .collect();

            ProjectedMacroSurfaces {
                native_props,
                props,
                emits: Vec::new(),
                slots: Vec::new(),
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
                    AnalyzedEmitField {
                        name: emit.name.clone(),
                        span: verter_span::Span::default(),
                        payload_type,
                        description,
                        tags,
                        payload_expr: None,
                        payload_expr_scope: None,
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
                    emits.push(AnalyzedEmitField {
                        name,
                        span: verter_span::Span::default(),
                        payload_type,
                        description,
                        tags,
                        payload_expr: None,
                        payload_expr_scope: None,
                    });
                }
            }

            ProjectedMacroSurfaces {
                native_props,
                props: Vec::new(),
                emits,
                slots: Vec::new(),
                ..Default::default()
            }
        }
        AnalyzedMacroKind::DefineSlots => {
            let slots = elements
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
                    Some(AnalyzedSlotField {
                        name,
                        is_required: !prop.optional,
                        span: verter_span::Span::default(),
                        bindings,
                        return_type,
                        description,
                        tags,
                        return_expr: None,
                        return_expr_scope: None,
                    })
                })
                .collect();

            ProjectedMacroSurfaces {
                native_props,
                props: Vec::new(),
                emits: Vec::new(),
                slots,
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

#[cfg(test)]
mod tests {
    use super::*;
    use verter_compiler::utils::oxc::vue::resolve_type::{ResolvedEmit, ResolvedProp};
    use verter_semantic::analysis::TypeResolutionSource;

    fn prop(
        name: &str,
        optional: bool,
        visibility: ResolvedMemberVisibility,
        type_text: Option<&str>,
        span_start: u32,
    ) -> ResolvedProp {
        ResolvedProp {
            span: verter_span::Span::new(span_start, span_start + 8),
            key: verter_span::Span::new(span_start, span_start + 3),
            key_name: Some(name.to_string()),
            optional,
            types: Vec::new(),
            visibility,
            type_span: None,
            type_text: type_text.map(str::to_string),
            map_local: false,
            span_is_absolute: true,
            type_expr: None,
            type_expr_scope: None,
        }
    }

    fn prop_with_type_span(
        name: &str,
        optional: bool,
        visibility: ResolvedMemberVisibility,
        type_text: Option<&str>,
        span: verter_span::Span,
        key: verter_span::Span,
        type_span: verter_span::Span,
    ) -> ResolvedProp {
        ResolvedProp {
            span,
            key,
            key_name: Some(name.to_string()),
            optional,
            types: Vec::new(),
            visibility,
            type_span: Some(type_span),
            type_text: type_text.map(str::to_string),
            map_local: false,
            span_is_absolute: true,
            type_expr: None,
            type_expr_scope: None,
        }
    }

    #[test]
    fn project_define_props_filters_non_public_members() {
        let elements = ResolvedElements {
            props: vec![
                prop(
                    "label",
                    false,
                    ResolvedMemberVisibility::Public,
                    Some("string"),
                    0,
                ),
                prop(
                    "secret",
                    true,
                    ResolvedMemberVisibility::Private,
                    Some("number"),
                    10,
                ),
            ],
            ..ResolvedElements::default()
        };

        let projected = project_macro_surfaces(None, AnalyzedMacroKind::DefineProps, &elements);

        assert_eq!(projected.native_props.len(), 2);
        assert_eq!(projected.props.len(), 1);
        assert_eq!(projected.props[0].name, "label");
        assert_eq!(
            projected.props[0].resolution_source,
            TypeResolutionSource::Rust
        );
    }

    #[test]
    fn project_define_emits_formats_payloads() {
        let elements = ResolvedElements {
            emits: vec![
                ResolvedEmit {
                    span: verter_span::Span::new(0, 5),
                    name: "save".to_string(),
                    name_span: None,
                    signature: ResolvedEmitSignature::Call {
                        params_text: "value: string".to_string(),
                    },
                    map_local: false,
                    span_is_absolute: true,
                    type_expr: None,
                    type_expr_scope: None,
                },
                ResolvedEmit {
                    span: verter_span::Span::new(6, 12),
                    name: "cancel".to_string(),
                    name_span: None,
                    signature: ResolvedEmitSignature::Tuple {
                        tuple_text: "[reason: number]".to_string(),
                    },
                    map_local: false,
                    span_is_absolute: true,
                    type_expr: None,
                    type_expr_scope: None,
                },
            ],
            ..ResolvedElements::default()
        };

        let projected = project_macro_surfaces(None, AnalyzedMacroKind::DefineEmits, &elements);

        assert_eq!(projected.emits.len(), 2);
        assert_eq!(
            projected.emits[0].payload_type.as_deref(),
            Some("[value: string]")
        );
        assert_eq!(
            projected.emits[1].payload_type.as_deref(),
            Some("[reason: number]")
        );
    }

    #[test]
    fn project_define_props_prefers_raw_source_type_span_text() {
        let source = "interface Props { type?: SingleOrMultipleType }";
        let type_start = source.find("SingleOrMultipleType").unwrap() as u32;
        let prop_start = source.find("type?").unwrap() as u32;
        let elements = ResolvedElements {
            props: vec![prop_with_type_span(
                "type",
                true,
                ResolvedMemberVisibility::Public,
                None,
                verter_span::Span::new(prop_start, source.len() as u32 - 2),
                verter_span::Span::new(prop_start, prop_start + 4),
                verter_span::Span::new(
                    type_start,
                    type_start + "SingleOrMultipleType".len() as u32,
                ),
            )],
            ..ResolvedElements::default()
        };

        let projected =
            project_macro_surfaces(Some(source), AnalyzedMacroKind::DefineProps, &elements);

        assert_eq!(
            projected.props[0].type_annotation.as_deref(),
            Some("SingleOrMultipleType")
        );
    }

    #[test]
    fn project_define_props_prefers_pre_resolved_cross_file_type_text_over_source_span() {
        let source = r#"export interface ButtonProps {
  /**
   * @defaultValue 'md'
   */
  size?: Button['variants']['size']
}"#;
        let type_start = source.find("'md'").unwrap() as u32;
        let prop_start = source.find("size?").unwrap() as u32;
        let elements = ResolvedElements {
            props: vec![prop_with_type_span(
                "size",
                true,
                ResolvedMemberVisibility::Public,
                Some("Button['variants']['size']"),
                verter_span::Span::new(prop_start, source.len() as u32 - 2),
                verter_span::Span::new(prop_start, prop_start + 4),
                verter_span::Span::new(type_start, type_start + 4),
            )],
            ..ResolvedElements::default()
        };

        let projected =
            project_macro_surfaces(Some(source), AnalyzedMacroKind::DefineProps, &elements);

        assert_eq!(
            projected.props[0].type_annotation.as_deref(),
            Some("Button['variants']['size']")
        );
    }

    #[test]
    fn project_define_emits_prefers_raw_source_tuple_payload_text() {
        let source =
            "type Emits = { 'update:modelValue': [value: (T extends 'single' ? string : string[]) | undefined]; }";
        let emit_start = source.find("'update:modelValue'").unwrap() as u32;
        let emit_end = source[emit_start as usize..].find(';').unwrap() as u32 + emit_start;
        let elements = ResolvedElements {
            emits: vec![ResolvedEmit {
                span: verter_span::Span::new(emit_start, emit_end),
                name: "update:modelValue".to_string(),
                name_span: None,
                signature: ResolvedEmitSignature::Tuple {
                    tuple_text: "[value: string | string[] | undefined]".to_string(),
                },
                map_local: false,
                span_is_absolute: true,
                type_expr: None,
                type_expr_scope: None,
            }],
            ..ResolvedElements::default()
        };

        let projected =
            project_macro_surfaces(Some(source), AnalyzedMacroKind::DefineEmits, &elements);

        assert_eq!(
            projected.emits[0].payload_type.as_deref(),
            Some("[value: (T extends 'single' ? string : string[]) | undefined]")
        );
    }

    #[test]
    fn project_define_slots_extracts_bindings_and_return_type() {
        let elements = ResolvedElements {
            props: vec![prop(
                "default",
                false,
                ResolvedMemberVisibility::Public,
                Some("(props: { foo: string; bar?: number }) => VNode[]"),
                0,
            )],
            ..ResolvedElements::default()
        };

        let projected = project_macro_surfaces(None, AnalyzedMacroKind::DefineSlots, &elements);

        assert_eq!(projected.slots.len(), 1);
        assert_eq!(projected.slots[0].name, "default");
        assert_eq!(projected.slots[0].bindings.len(), 2);
        assert_eq!(projected.slots[0].bindings[0].name, "foo");
        assert_eq!(projected.slots[0].bindings[1].name, "bar");
        assert_eq!(projected.slots[0].return_type.as_deref(), Some("VNode[]"));
    }

    #[test]
    fn project_define_slots_preserves_symbolic_binding_types_for_pick_params() {
        let source = r#"
type CalendarCellTriggerProps = { day: string; month: number }
export interface Slots {
  day?: (props: Pick<CalendarCellTriggerProps, 'day'>) => any
}
"#;
        let elements = ResolvedElements {
            props: vec![prop(
                "day",
                true,
                ResolvedMemberVisibility::Public,
                Some("(props: Pick<CalendarCellTriggerProps, 'day'>) => any"),
                0,
            )],
            ..ResolvedElements::default()
        };

        let projected =
            project_macro_surfaces(Some(source), AnalyzedMacroKind::DefineSlots, &elements);

        assert_eq!(projected.slots.len(), 1);
        assert_eq!(projected.slots[0].bindings.len(), 1);
        assert_eq!(projected.slots[0].bindings[0].name, "day");
        assert_eq!(
            projected.slots[0].bindings[0].type_annotation.as_deref(),
            Some("CalendarCellTriggerProps['day']")
        );
    }

    // the
    // `project_expanded_text_define_emits_preserves_conditional_payload_text`
    // and `project_local_source_define_slots_preserves_symbolic_pick_binding`
    // unit tests were attached to the (now-deleted) text-based
    // projector helpers. Their behaviour contracts are now covered
    // by integration tests in `meta_resolve_tests` and
    // `component_meta_audit`.

    #[test]
    fn project_define_slots_ignores_non_callable_helper_members() {
        let elements = ResolvedElements {
            props: vec![
                prop(
                    "default",
                    false,
                    ResolvedMemberVisibility::Public,
                    Some("(props: { item: string }) => any"),
                    0,
                ),
                prop(
                    "appConfig",
                    false,
                    ResolvedMemberVisibility::Public,
                    Some("{ ui?: { variant: string } }"),
                    0,
                ),
                prop(
                    "slots",
                    false,
                    ResolvedMemberVisibility::Public,
                    Some("{ leading?: string; trailing?: string }"),
                    0,
                ),
            ],
            ..ResolvedElements::default()
        };

        let projected = project_macro_surfaces(None, AnalyzedMacroKind::DefineSlots, &elements);
        let names: Vec<_> = projected
            .slots
            .iter()
            .map(|slot| slot.name.as_str())
            .collect();

        assert_eq!(names, vec!["default"]);
    }

    // the `project_local_source_define_props_*` tests
    // exercised the (now-deleted) source-typed projector. The
    // behaviour contracts they covered (heritage resolution, JSDoc
    // through `@vue-ignore`-annotated `Omit<>`) are covered by
    // integration tests in `meta_resolve_tests` and `meta_tests`.
}
