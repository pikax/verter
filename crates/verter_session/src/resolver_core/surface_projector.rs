use verter_compiler::utils::oxc::vue::resolve_type::{
    AnalyzedExternalTypeSource, ResolvedElements, ResolvedMemberVisibility,
};
use verter_semantic::analysis::types::AnalyzedMacroKind;
use verter_type_expr::TypeExpr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedNativeProp {
    pub name: String,
    pub is_optional: bool,
    pub type_annotation: Option<String>,
    pub visibility: ResolvedMemberVisibility,
    pub span: verter_span::Span,
}

/// The native-only macro surface carrier.
///
/// Holds the private/protected class-member visibility surface
/// (`native_props`) projected from the eager OXC [`ResolvedElements`] for an
/// imported macro type. This is the SOLE responsibility of the surface
/// projector after the props/emits/slots cutover: the published
/// props/emits/slots surface is owned exclusively by the typeinfo macro-surface
/// path ([`crate::VerterHost::vue_macro_dtos`] → the
/// `props/emits/slots_from_typeinfo_surface` normalizers). `native_props` has a
/// real FFI/proto/JS consumer (`@verter/component-meta` `nativeProps`) that the
/// typeinfo surface does NOT cover, so it stays here as a native-only carrier.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectedMacroSurfaces {
    pub native_props: Vec<ResolvedNativeProp>,
}

pub fn project_macro_surfaces(
    source: Option<&str>,
    macro_kind: AnalyzedMacroKind,
    elements: &ResolvedElements,
) -> ProjectedMacroSurfaces {
    project_macro_surfaces_with_owner(source, None, macro_kind, elements)
}

/// Project the native-only macro surface ([`ProjectedMacroSurfaces`]) from the
/// eager OXC [`ResolvedElements`].
///
/// After the props/emits/slots cutover this projector is responsible for ONLY
/// `native_props` (the private/protected class-member visibility surface). The
/// published props/emits/slots are owned exclusively by the typeinfo
/// macro-surface path; `source`, `owner_canonical`, and `macro_kind` no longer
/// participate in the projection and are retained on the signature for the
/// existing caller ([`project_macro_surfaces`], reached from
/// `component_meta::cold_resolver`) without behavioural effect.
pub fn project_macro_surfaces_with_owner(
    source: Option<&str>,
    owner_canonical: Option<&str>,
    macro_kind: AnalyzedMacroKind,
    elements: &ResolvedElements,
) -> ProjectedMacroSurfaces {
    let _ = (source, owner_canonical, macro_kind);
    ProjectedMacroSurfaces {
        native_props: collect_native_props(elements),
    }
}

/// When a type is not locally defined in a file (e.g., barrel re-export),
/// find its import source specifiers and imported name so the caller can follow
/// the import chain. Handles `import { T } from '...'; export { T };`,
/// `export { T } from '...'`, and `export * from '...'` patterns.
///
/// Reads the cache-owned shallow analysis ([`AnalyzedExternalTypeSource`], held
/// on the file's [`crate::resolver_core::ShallowFileState`]); it NEVER allocates
/// a fresh OXC arena or reparses the dependency. The barrel-discovery overlay
/// fetches the shallow state once per canonical via
/// [`crate::VerterHost::shallow_file_state`] and passes the cache-owned analysis
/// here.
///
/// Returns all candidates. For direct/named re-exports returns one entry.
/// For `export *` wildcards returns one entry per wildcard source.
pub fn find_type_import_sources_in_analysis(
    analysis: &AnalyzedExternalTypeSource,
    type_name: &str,
) -> Vec<(String, String)> {
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

/// Given a file's cache-owned shallow analysis and a locally-defined type name,
/// return the imported type dependencies that the type transitively references
/// through its heritage chain (extends, intersection, etc.).
///
/// Reads the cache-owned shallow analysis ([`AnalyzedExternalTypeSource`]); it
/// NEVER reparses. Each entry is `(import_specifier, imported_name)` — e.g.,
/// `("vue-router", "RouterLinkProps")` for a type that extends an imported
/// interface.
pub fn find_heritage_type_imports_in_analysis(
    analysis: &AnalyzedExternalTypeSource,
    type_name: &str,
) -> Vec<(String, String)> {
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
pub(crate) fn render_type_expr_display(expr: &TypeExpr) -> Option<String> {
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
