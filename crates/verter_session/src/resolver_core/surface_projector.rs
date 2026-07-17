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
/// Uses `verter_type_expr`'s heap-worklist renderer: deep finite types have no
/// structural-depth cap and do not consume one Rust call frame per type node.
/// A typed rendering error stays the `None` display signal; the typed carrier
/// remains authoritative and is never converted to a fabricated `unknown`.
pub(crate) fn render_type_expr_display(expr: &TypeExpr) -> Option<String> {
    verter_type_expr::render_type_expr_display(expr)
        .ok()
        .map(|rendered| rendered.text)
}
