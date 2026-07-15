use verter_parser::utils::oxc::script::type_surface::ResolvedElements;
use verter_type_expr::{MemberVisibility, TypeExpr};

use crate::typeinfo::surface::TypeInfoSurfaceMember;

/// One keep-all class-member visibility row published to the
/// `@verter/component-meta` `nativeProps` consumer (FFI/proto/JS).
///
/// Built DIRECTLY from the shared-dispatch one-level surface members
/// ([`TypeInfoSurfaceMember`]) by `ResolvedNativeProp::from_surface_member`
/// (below), invoked from the vue_exec projection terminal
/// `macro_elements_from_surface`
/// (`typeinfo/framework_surface/vue_exec/imported_elements.rs`) — never
/// from a parser DTO round-trip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedNativeProp {
    pub name: String,
    pub is_optional: bool,
    pub type_annotation: Option<String>,
    /// The member's declared accessibility, carried VERBATIM from the
    /// surface member's canonical `verter_type_expr::MemberVisibility` —
    /// every visibility is retained (public AND protected AND private);
    /// this surface never applies the publication-boundary `Public`-only
    /// filter.
    pub visibility: MemberVisibility,
    /// Always `Span::default()`. The FFI/proto wire row
    /// (`FfiResolvedNativeProp` / proto `ResolvedNativeProp`) emits only
    /// `span_start` / `span_end` with NO declaration-file id, and an
    /// inherited member's declaration site lives in ITS OWN file — a bare
    /// byte offset would be an unanchored index into an unnamed file,
    /// misleading rather than useful. The honest 0,0 default is the wire
    /// contract; real span fidelity requires the wire to carry a
    /// declaration-file anchor alongside the offsets.
    pub span: verter_span::Span,
}

/// The dispatch-resolved macro-elements payload for one imported macro type:
/// the legacy compile-facing [`ResolvedElements`] DTO plus the keep-all
/// `native_props` rows, BOTH projected from the SAME one-level
/// `TypeInfoSurface` resolution (one resolution, two projections — the
/// native rows are never re-derived from the elements DTO and never come
/// from a separate re-resolve).
///
/// `native_props` has a real FFI/proto/JS consumer
/// (`@verter/component-meta` `nativeProps`) that the published typeinfo
/// surface does NOT cover: it is the class-member visibility surface
/// (public AND protected AND private, visibility carried, not filtered).
#[derive(Debug, Clone)]
pub struct ResolvedMacroElements {
    pub elements: ResolvedElements,
    pub native_props: Vec<ResolvedNativeProp>,
}

impl ResolvedNativeProp {
    /// Build one keep-all row DIRECTLY from a resolved surface member — the
    /// SOLE production constructor for `native_props` rows.
    ///
    /// Every visibility maps verbatim (this constructor never coerces or
    /// filters accessibility), and there is no macro-kind input
    /// (kind-independence is structural: the member is the whole input).
    /// `type_annotation` is supplied by the caller, rendered ONCE from the
    /// member's raised value at the vue_exec projection terminal
    /// (`macro_elements_from_surface`) so both the elements DTO and the
    /// native row read the same rendered text — a display publication the
    /// constructor stores, never reads. `span` is unconditionally
    /// `Span::default()` — see the field doc: the wire carries no
    /// declaration-file anchor, so declaration-site offsets would be
    /// unanchored.
    pub(crate) fn from_surface_member(
        member: &TypeInfoSurfaceMember,
        type_annotation: Option<String>,
    ) -> Self {
        Self {
            name: member.name.as_ref().to_string(),
            is_optional: member.optional,
            type_annotation,
            visibility: member.visibility,
            span: verter_span::Span::default(),
        }
    }
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
