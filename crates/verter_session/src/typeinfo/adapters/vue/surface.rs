#![deny(missing_docs)]
//! The `.vue` FullMetadata macro surface + the prop / emit / slot normalizers.
//!
//! [`resolve_vue_macro_surface`] resolves ONE `.vue` macro's type-argument
//! surface (`defineProps<T>()` / `defineEmits<E>()` / `defineSlots<S>()` /
//! `withDefaults(defineProps<T>(), …)`) to the span-rich [`VueMacroSurface`]
//! at [`TypeInfoQueryLevel::FullMetadata`]. The surface is sourced through the
//! SHARED typeinfo surface path — the macro type-argument is lowered through
//! the shared lowering dispatch and projected by the SAME empty-path `Shallow`
//! synthesiser [`crate::VerterHost::resolve_shallow_surface_for`] uses. It is
//! NEVER read from `surface_view_from_base_node` (the parallel reader U3c
//! deletes).
//!
//! The three normalizers ([`props_from_typeinfo_surface`] /
//! [`emits_from_typeinfo_surface`] / [`slots_from_typeinfo_surface`]) consume a
//! [`crate::typeinfo::surface::TypeInfoSurface`] (member `value` +
//! spans + origin + flags + JSDoc spans) plus the macro-analyzer facts and
//! produce the FINAL component-meta DTOs (`AnalyzedPropField` /
//! `AnalyzedEmitField` / `AnalyzedSlotField`). They reproduce the eager rail's
//! behavior (the `ImportedMacroSurface::LazyImported` arm +
//! `surface_projector`) member-for-member, sourcing every semantic decision
//! from the typeinfo surface:
//!
//! - **props** — one field per named member, carrying the surface's `optional`
//!   / `readonly` (RICHER than the eager rail's hardcoded `false` — taken from
//!   the surface) / `declared_in_macro_type_arg`, the member value raised to a
//!   `TypeExpr` (scoped to the member's declaration-origin file), the
//!   `defineModel` synthesized model prop from analyzer facts, and JSDoc sliced
//!   from the surface's JSDoc SPANS.
//! - **emits** — call-signature event extraction FIRST (the first parameter's
//!   string-literal — or union of string literals — is the event name; the
//!   payload is the call-signature function with the leading event-name
//!   parameter STRIPPED), property-key members only as a fallback when no
//!   call-signature emit was found, de-duplicated by event name
//!   (first-writer-wins).
//! - **slots** — function-like members only (non-function members filtered);
//!   the first-parameter object's properties become the slot bindings; the
//!   function return type becomes the slot return.
//!
//! Fallthrough / root-inheritance + expose / options are SEPARATE subsystems
//! fed by analyzer facts — out of scope here.

use std::sync::Arc;

use verter_semantic::analysis::types::{
    AnalyzedEmitField, AnalyzedMacroKind, AnalyzedPropField, AnalyzedSlotField,
    AnalyzedSlotFieldBinding, JsdocTag,
};
use verter_type_expr::{LiteralValue, TypeExpr, TypeExprScope};

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::surface_projector::render_type_expr_display;
use crate::semantic_query::{ProjectionMode, ProjectionReductionContext};
use crate::typeinfo::adapters::vue::store::{VueMacroDtoKey, VueMacroDtos};
use crate::typeinfo::surface::{CanonicalSpan, TypeInfoSurface, TypeInfoSurfaceMember};
use crate::typeinfo::types::{TypeInfoQueryLevel, VueMacroSurfaceRequest};
use crate::VerterHost;

/// A typeinfo-owned `.vue` macro surface (FullMetadata).
///
/// Carries the span-rich [`TypeInfoSurface`] for ONE macro's type argument
/// plus the macro's kind, declaration identity, and the SFC scope the macro
/// was written in. Like the surface itself it holds NO owned type / JSDoc text
/// — only the surface (spans + ids + flags), the macro kind, and interned
/// `Arc<str>` scope / canonical ids. A consumer slices source on demand at the
/// FFI boundary; the normalizers in this module do that slicing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VueMacroSurface {
    /// The macro's span-rich one-level surface (the type argument's members /
    /// call signatures / index signatures).
    pub surface: TypeInfoSurface,
    /// Which macro produced this surface.
    pub macro_kind: AnalyzedMacroKind,
    /// Canonical id of the `.vue` SFC that declares the macro.
    pub owner_canonical: Arc<str>,
    /// Stable index of the macro in the SFC's analysis snapshot.
    pub macro_index: usize,
    /// SFC-absolute span of the macro CALL (from the analyzer fact). The
    /// surface members carry their own per-member declaration spans; this is
    /// the macro call site itself.
    pub macro_call_span: verter_span::Span,
    /// The query level this surface was resolved at (always
    /// [`TypeInfoQueryLevel::FullMetadata`] for a macro surface).
    pub level: TypeInfoQueryLevel,
}

impl VueMacroSurface {
    /// The scope a member's raised `*_expr` should bind to — the member's
    /// DECLARATION-origin file (which U1 made survive substitution), falling
    /// back to the SFC owner when the member carries no single-file origin (a
    /// synthetic / structural member). Mirrors
    /// `ImportedMacroSurface::member_expr_scope`, sourced from the typeinfo
    /// surface's `origin.canonical_file` instead of a value-node scope lookup.
    fn member_expr_scope(&self, member: &TypeInfoSurfaceMember) -> TypeExprScope {
        member
            .origin
            .canonical_file
            .as_ref()
            .map(|canonical| TypeExprScope::new(canonical.as_ref()))
            .unwrap_or_else(|| TypeExprScope::new(self.owner_canonical.as_ref()))
    }
}

impl VerterHost {
    /// Resolve a `.vue` macro's type-argument surface to its span-rich
    /// [`VueMacroSurface`] (FullMetadata) through the shared typeinfo surface
    /// path.
    ///
    /// Returns `None` when the SFC is not loaded, the macro index is out of
    /// range, the macro is not type-based / has no parsed type argument, or the
    /// type argument does not project to an object surface (a macro typed as a
    /// primitive / union has no one-level member surface).
    ///
    /// **Provenance:** a props macro (`DefineProps` / `WithDefaults`) lowers
    /// its type argument under
    /// [`ProjectionReductionContext::published_macro_type_arg_body`] so the
    /// type-argument's OWN-body members surface with
    /// `declared_in_macro_type_arg = true` and heritage-reached members stay
    /// `false`. Structural macros (`DefineEmits` / `DefineSlots`) lower under
    /// the structural `published` context (`declared_in_macro_type_arg` is a
    /// props-axis concern).
    #[must_use]
    pub fn resolve_vue_macro_surface(
        &self,
        request: &VueMacroSurfaceRequest,
    ) -> Option<VueMacroSurface> {
        debug_assert_eq!(
            request.level,
            TypeInfoQueryLevel::FullMetadata,
            "resolve_vue_macro_surface serves the FullMetadata level"
        );

        let indexed = self.ensure_indexed_ready(request.owner_canonical.as_ref())?;
        let mac = indexed.snapshot.macros.get(request.macro_index)?;
        if !mac.is_type_based {
            return None;
        }

        // `defineModel` does NOT carry a props OBJECT type argument — its type
        // argument is the model VALUE type (`defineModel<string>()`), which has
        // no one-level member surface. Its props come from the analyzer-
        // synthesized model prop (`AnalyzedMacro.prop_fields`), so the macro
        // surface is the EMPTY object surface; `props_from_typeinfo_surface`
        // routes `DefineModel` to the analyzer-fact path.
        if request.macro_kind == AnalyzedMacroKind::DefineModel {
            return Some(VueMacroSurface {
                surface: TypeInfoSurface::empty(),
                macro_kind: request.macro_kind,
                owner_canonical: Arc::clone(&request.owner_canonical),
                macro_index: request.macro_index,
                macro_call_span: mac.span,
                level: request.level,
            });
        }

        let type_arg = mac.parsed_type_argument.as_ref()?;

        // Provenance per macro axis. Props carry macro-T own-body provenance so
        // the author-declared members are flagged; emits / slots are structural.
        let context = match request.macro_kind {
            AnalyzedMacroKind::DefineProps | AnalyzedMacroKind::WithDefaults => {
                ProjectionReductionContext::published_macro_type_arg_body(ProjectionMode::Shallow)
            }
            _ => ProjectionReductionContext::published(ProjectionMode::Shallow),
        };

        let store_view = self.resolver_store_view();
        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx = crate::resolver_core::HostResolverContext::new(self, &store_view, overlay);
        let dispatch = ProjectSemanticDispatch::new(&host_ctx);

        // Lower the macro type argument in the SFC scope. `Navigate` lowering
        // keeps member values shallow; the empty-path `Shallow` projection then
        // synthesises the one-level surface under `context`.
        let base = dispatch.lower_type_expr_in_scope_with_context(
            request.owner_canonical.as_ref(),
            type_arg,
            ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
        )?;

        let surface =
            self.project_shallow_surface_from_base(&host_ctx, &dispatch, base, context)?;

        Some(VueMacroSurface {
            surface,
            macro_kind: request.macro_kind,
            owner_canonical: Arc::clone(&request.owner_canonical),
            macro_index: request.macro_index,
            macro_call_span: mac.span,
            level: request.level,
        })
    }

    /// Resolve a `.vue` macro's NORMALIZED component-meta DTOs
    /// ([`VueMacroDtos`]), consulting the host-owned
    /// [`crate::typeinfo::adapters::vue::store::VueShallowMetadataStore`] first.
    ///
    /// This is the cached FullMetadata entry point: it materializes the macro
    /// surface ONCE per `(canonical, content, macro, level)` (resolving the
    /// surface and running the appropriate normalizer), publishes the immutable
    /// owned DTO bundle into the store, and serves subsequent calls from the
    /// content-addressed cache. The DTO bundle is generation-independent (owned
    /// `TypeExpr` + scope + `String`), so caching it across requests is safe.
    ///
    /// Returns an empty (default) bundle when the macro surface cannot be
    /// resolved — the same "no surface" outcome the eager rail produces for an
    /// unresolvable macro. The bundle is still cached so a repeat request does
    /// not re-attempt the cold resolution.
    #[must_use]
    pub fn vue_macro_dtos(&self, request: &VueMacroSurfaceRequest) -> Arc<VueMacroDtos> {
        let key = VueMacroDtoKey::new(
            Arc::clone(&request.owner_canonical),
            request.root_identity,
            request.macro_index,
            request.level,
        );
        if let Some(cached) = self.vue_shallow_metadata_store().get(&key) {
            return cached;
        }

        let dtos = match self.resolve_vue_macro_surface(request) {
            Some(macro_surface) => match request.macro_kind {
                AnalyzedMacroKind::DefineProps
                | AnalyzedMacroKind::WithDefaults
                | AnalyzedMacroKind::DefineModel => VueMacroDtos {
                    props: props_from_typeinfo_surface(self, &macro_surface),
                    ..VueMacroDtos::default()
                },
                AnalyzedMacroKind::DefineEmits => VueMacroDtos {
                    emits: emits_from_typeinfo_surface(self, &macro_surface),
                    ..VueMacroDtos::default()
                },
                AnalyzedMacroKind::DefineSlots => VueMacroDtos {
                    slots: slots_from_typeinfo_surface(self, &macro_surface),
                    ..VueMacroDtos::default()
                },
                // Options / expose are separate subsystems — no DTO bundle.
                AnalyzedMacroKind::DefineOptions | AnalyzedMacroKind::DefineExpose => {
                    VueMacroDtos::default()
                }
            },
            None => VueMacroDtos::default(),
        };

        self.vue_shallow_metadata_store().get_or_insert(key, dtos)
    }
}

/// Slice a member's leading-JSDoc DESCRIPTION + TAG spans into owned text for
/// the published DTO. The spans are already located on the surface (by U1's
/// `with_member_jsdoc_spans`); this reads the declaring file's cache-owned
/// source and slices — it does NOT re-locate the comment block and does NOT
/// take the lazy `member_display_jsdoc` name-search path.
///
/// Returns `(None, empty)` when the member carries no JSDoc spans or the
/// declaring file's source is unavailable.
fn member_jsdoc_from_spans(
    host: &VerterHost,
    member: &TypeInfoSurfaceMember,
) -> (Option<String>, Vec<JsdocTag>) {
    let slice = |cspan: &CanonicalSpan| -> Option<String> {
        let indexed = host.ensure_indexed_ready(cspan.file.as_ref())?;
        let source = Arc::clone(&indexed.eval_source);
        let start = cspan.span.start as usize;
        let end = cspan.span.end as usize;
        source.get(start..end).map(|s| s.to_string())
    };

    let description = member
        .jsdoc_description_span
        .as_ref()
        .and_then(&slice)
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty());

    let tags: Vec<JsdocTag> = member
        .jsdoc_tag_spans
        .iter()
        .filter_map(|tag| {
            let name = slice(&tag.name_span)?.trim().to_string();
            if name.is_empty() {
                return None;
            }
            let text = tag
                .text_span
                .as_ref()
                .and_then(&slice)
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty());
            Some(JsdocTag { name, text })
        })
        .collect();

    (description, tags)
}

/// Raise a member's value node to a [`TypeExpr`] through the shared structural
/// raiser. `None` when the node has no raisable shape (the caller substitutes
/// the eager rail's missing-`type_expr` fallback).
fn raise_member_value(host: &VerterHost, member: &TypeInfoSurfaceMember) -> Option<TypeExpr> {
    let store_view = host.resolver_store_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    dispatch.raise_node_to_type_expr(member.value)
}

/// Normalize a `.vue` props macro surface into the published
/// [`AnalyzedPropField`] set.
///
/// Reproduces the eager rail's `AnalyzedPropField` stream
/// (`surface_projector::project_macro_surfaces` for the local SFC,
/// `ImportedMacroSurface::prop_members` for cross-file) over the typeinfo
/// surface: one field per named member, carrying the surface's `optional` /
/// `readonly` / `declared_in_macro_type_arg`, the member value raised to a
/// `TypeExpr` scoped to its declaration-origin file, the display
/// `type_annotation` rendered from that typed form, and JSDoc sliced from the
/// surface spans. Own-body-vs-heritage ordering + shadowing + union-common
/// membership are ALREADY resolved on the surface (the merge ran in the shared
/// projector) — this is a thin per-member transform.
///
/// `defineModel` does NOT carry an object type argument (its type argument is
/// the MODEL value type), so its surface has no named members; the synthesized
/// model prop is appended from the analyzer facts
/// ([`AnalyzedMacroKind::DefineModel`]'s `prop_fields`). The optionality the
/// eager rail derives from `withDefaults` / `defineModel` defaults is applied
/// DOWNSTREAM by the component-meta projection (`PropAnalysis.required` /
/// `has_default`), NOT on `AnalyzedPropField`, so the field's `is_optional`
/// here stays the RAW type-argument optionality — matching the eager rail.
#[must_use]
pub fn props_from_typeinfo_surface(
    host: &VerterHost,
    macro_surface: &VueMacroSurface,
) -> Vec<AnalyzedPropField> {
    // `defineModel` contributes its synthesized model prop directly from the
    // analyzer facts (the type argument is the model VALUE type, not a props
    // object). Source from `AnalyzedMacro.prop_fields` (populated by the
    // analyzer's `extract_define_model_type`) — the model prop is genuinely
    // analyzer-derived, not a macro-T object surface member.
    if macro_surface.macro_kind == AnalyzedMacroKind::DefineModel {
        return model_prop_fields(host, macro_surface);
    }

    macro_surface
        .surface
        .members
        .iter()
        .map(|member| {
            let type_expr = raise_member_value(host, member);
            let type_expr_scope = type_expr
                .as_ref()
                .map(|_| macro_surface.member_expr_scope(member));
            let type_annotation = type_expr.as_ref().and_then(render_type_expr_display);
            let (description, tags) = member_jsdoc_from_spans(host, member);
            AnalyzedPropField {
                name: member.name.as_ref().to_string(),
                is_optional: member.optional,
                span: verter_span::Span::default(),
                type_annotation,
                type_expr,
                type_expr_scope,
                description,
                tags,
                resolution_source: verter_semantic::analysis::types::TypeResolutionSource::Rust,
                resolution_error: None,
                declared_in_macro_type_arg: member.declared_in_macro_type_arg,
            }
        })
        .collect()
}

/// Build the `defineModel` synthesized prop field from the analyzer facts.
/// `defineModel<T>('name', { … })` synthesizes a prop named `name`
/// (default `modelValue`) typed `T`; the analyzer already captured this as the
/// macro's single `prop_fields` entry. Re-scope the typed form to the SFC owner
/// so nested `Ref`s resolve in the SFC.
fn model_prop_fields(host: &VerterHost, macro_surface: &VueMacroSurface) -> Vec<AnalyzedPropField> {
    let Some(indexed) = host.ensure_indexed_ready(macro_surface.owner_canonical.as_ref()) else {
        return Vec::new();
    };
    let Some(mac) = indexed.snapshot.macros.get(macro_surface.macro_index) else {
        return Vec::new();
    };
    mac.prop_fields
        .iter()
        .map(|field| {
            // The analyzer stamps an empty scope on the synthesized model prop;
            // re-anchor it to the SFC owner so the pairing invariant holds with
            // a real scope.
            let type_expr_scope = field
                .type_expr
                .as_ref()
                .map(|_| TypeExprScope::new(macro_surface.owner_canonical.as_ref()));
            AnalyzedPropField {
                type_expr_scope,
                ..field.clone()
            }
        })
        .collect()
}

/// Normalize a `.vue` emits macro surface into the published
/// [`AnalyzedEmitField`] set.
///
/// Reproduces `ImportedMacroSurface::emit_members` over the typeinfo surface:
///
/// 1. **Call-signature emits FIRST.** Each call signature's first parameter is
///    the event name (a `String` literal, or a `Union` of `String` literals);
///    the payload is the call-signature function with the leading event-name
///    parameter STRIPPED (`(e: 'change', v: number) => void` → event `change`,
///    payload `(v: number) => void`). The event name is NEVER read from `keyof`
///    (which would surface numeric tuple indices).
/// 2. **Property-style emits as a FALLBACK** — only when no call-signature emit
///    was found. Each named member is an event; its value type is the payload.
/// 3. **De-duplicate by event name, first-writer-wins** (matching the eager
///    projector's `retain`).
#[must_use]
pub fn emits_from_typeinfo_surface(
    host: &VerterHost,
    macro_surface: &VueMacroSurface,
) -> Vec<AnalyzedEmitField> {
    let store_view = host.resolver_store_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);

    let mut emits: Vec<AnalyzedEmitField> = Vec::new();

    // (1) Call-signature emits.
    for sig in macro_surface.surface.call_signatures.iter() {
        let Some(TypeExpr::Function(func)) = dispatch.raise_node_to_type_expr(sig.node) else {
            continue;
        };
        let Some(first) = func.parameters.first() else {
            continue;
        };
        // Payload = the call signature with the leading event-name parameter
        // dropped. Preserves the function's spans + surviving parameter spans.
        let payload_fn = TypeExpr::Function(Arc::new(verter_type_expr::FunctionExpr::with_spans(
            func.parameters.iter().skip(1).cloned().collect(),
            func.return_type.clone(),
            func.type_parameters.clone(),
            func.spans,
        )));
        // Scope the payload to the SFC owner — the call signature was written in
        // the SFC (or, for an imported emit interface, lives in its own file;
        // the surface signature node carries no per-member origin, so the SFC
        // owner is the correct default scope, matching the eager local rail).
        let payload_scope = TypeExprScope::new(macro_surface.owner_canonical.as_ref());
        let mut push_event = |name: String| {
            emits.push(AnalyzedEmitField {
                name,
                span: verter_span::Span::default(),
                payload_type: render_type_expr_display(&payload_fn),
                payload_expr: Some(payload_fn.clone()),
                payload_expr_scope: Some(payload_scope.clone()),
                description: None,
                tags: Vec::new(),
            });
        };
        match &first.ty {
            TypeExpr::Literal(LiteralValue::String(name)) => push_event(name.clone()),
            TypeExpr::Union(types) => {
                for ty in types.iter() {
                    if let TypeExpr::Literal(LiteralValue::String(name)) = ty {
                        push_event(name.clone());
                    }
                }
            }
            _ => {}
        }
    }

    // (2) Property-style emits — fallback only when no call-signature emit fired.
    if emits.is_empty() {
        for member in macro_surface.surface.members.iter() {
            let payload_expr = raise_member_value(host, member);
            let payload_expr_scope = payload_expr
                .as_ref()
                .map(|_| macro_surface.member_expr_scope(member));
            let payload_type = payload_expr.as_ref().and_then(render_type_expr_display);
            let (description, tags) = member_jsdoc_from_spans(host, member);
            emits.push(AnalyzedEmitField {
                name: member.name.as_ref().to_string(),
                span: verter_span::Span::default(),
                payload_type,
                payload_expr,
                payload_expr_scope,
                description,
                tags,
            });
        }
    }

    // (3) De-duplicate by event name, first-writer-wins.
    let mut seen = std::collections::HashSet::new();
    emits.retain(|emit| seen.insert(emit.name.clone()));
    emits
}

/// Normalize a `.vue` slots macro surface into the published
/// [`AnalyzedSlotField`] set.
///
/// Reproduces `ImportedMacroSurface::slot_members` over the typeinfo surface:
/// keep FUNCTION-LIKE members only (the value raises to a `TypeExpr::Function`;
/// non-function members are filtered); the slot's `bindings` come from the
/// function's first-parameter object's properties; the `return_expr` /
/// `return_type` come from the function's return type. Bindings + return are
/// scoped to the slot member's declaration-origin file.
#[must_use]
pub fn slots_from_typeinfo_surface(
    host: &VerterHost,
    macro_surface: &VueMacroSurface,
) -> Vec<AnalyzedSlotField> {
    macro_surface
        .surface
        .members
        .iter()
        .filter_map(|member| {
            let value = raise_member_value(host, member)?;
            let func = match &value {
                TypeExpr::Function(func) => func,
                _ => return None,
            };
            let scope = macro_surface.member_expr_scope(member);
            let bindings = func
                .parameters
                .first()
                .map(|param| binding_fields_from_param_ty(&param.ty, &scope))
                .unwrap_or_default();
            let return_expr = func.return_type.as_ref().map(|rt| (**rt).clone());
            let return_expr_scope = return_expr.as_ref().map(|_| scope.clone());
            let return_type = return_expr.as_ref().and_then(render_type_expr_display);
            let (description, tags) = member_jsdoc_from_spans(host, member);
            Some(AnalyzedSlotField {
                name: member.name.as_ref().to_string(),
                is_required: !member.optional,
                span: verter_span::Span::default(),
                bindings,
                return_type,
                return_expr,
                return_expr_scope,
                description,
                tags,
            })
        })
        .collect()
}

/// Reconstruct a slot's binding fields from its function's first-parameter
/// object. Each object property becomes one [`AnalyzedSlotFieldBinding`]
/// carrying the property's value `TypeExpr` as `binding_expr`. A non-object
/// parameter yields no bindings — matching the eager rail. Mirrors
/// `imported_surface::binding_fields_from_param_ty`.
fn binding_fields_from_param_ty(
    param_ty: &TypeExpr,
    scope: &TypeExprScope,
) -> Vec<AnalyzedSlotFieldBinding> {
    let TypeExpr::Object(obj) = param_ty else {
        return Vec::new();
    };
    obj.properties
        .iter()
        .filter_map(|member| match member {
            verter_type_expr::ObjectMember::Property(prop) => Some(AnalyzedSlotFieldBinding {
                name: prop.name.clone(),
                type_annotation: render_type_expr_display(&prop.ty),
                binding_expr: Some(prop.ty.clone()),
                binding_expr_scope: Some(scope.clone()),
                span: verter_span::Span::default(),
            }),
            _ => None,
        })
        .collect()
}
