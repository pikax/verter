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
//! NEVER read from `surface_view_from_base_node` — the macro surface routes
//! through the one shared surface path, not a parallel reader.
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
//!   `TypeExpr` (scoped to the member's VALUE-NODE file, matching the eager
//!   rail — see [`VueMacroSurface::member_expr_scope`]), the `defineModel`
//!   synthesized model prop from analyzer facts, and JSDoc sliced from the
//!   surface's JSDoc SPANS.
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
    /// VALUE-NODE scope (`node_scope(member.value)` → file), matching the eager
    /// rail's `ImportedMacroSurface::member_expr_scope`.
    ///
    /// The value-node scope (NOT the member's declaration_origin) is the file
    /// whose OXC parse produced the typed value expression, which is where its
    /// nested `Ref`s must resolve. The two files DIVERGE for a generic
    /// inherited member: `interface Props extends Base<Local>` over an imported
    /// `Base<T> { val: T }` substitutes `val`'s value to `Local`, a node scoped
    /// to the DERIVING file (where `Local` lives), while the member's
    /// declaration_origin is the base file (where `val: T` is declared). Scoping
    /// the raised `*_expr` to the declaration_origin (base) would make
    /// `Ref("Local")` resolve in the wrong file — a cross-file Miss. JSDoc
    /// deliberately uses the declaration_origin instead (see
    /// [`member_jsdoc_from_spans`]); the two axes intentionally use different
    /// files, exactly as the eager rail does.
    ///
    /// Falls back to the member's declaration_origin, then the SFC owner, when
    /// the value node carries no single-file scope (a structural / scope-less
    /// value node — a primitive, a shared literal-union).
    fn member_expr_scope(
        &self,
        host: &VerterHost,
        member: &TypeInfoSurfaceMember,
    ) -> TypeExprScope {
        host.project_type_store()
            .semantic_graph()
            .node_scope(member.value)
            .and_then(|scope| scope.canonical_file())
            .map(|canonical| TypeExprScope::new(canonical.as_ref()))
            .or_else(|| {
                member
                    .origin
                    .canonical_file
                    .as_ref()
                    .map(|canonical| TypeExprScope::new(canonical.as_ref()))
            })
            .unwrap_or_else(|| TypeExprScope::new(self.owner_canonical.as_ref()))
    }

    /// The scope a call signature's stripped-payload `*_expr` should bind to.
    ///
    /// The payload is the signature's parameters AFTER the leading event-name
    /// parameter, so its scope follows the first PAYLOAD parameter's type-node
    /// scope (`node_scope(param.ty)` → file) — the file whose lowering produced
    /// that node, which is where the payload's `Ref`s must resolve. This matches
    /// the member axis ([`Self::member_expr_scope`], which scopes to the value
    /// node) and handles generic substitution correctly: `interface Emits
    /// extends TabsRootEmits<string | number>` over an imported `TabsRootEmits<T>
    /// { (e, payload: T): void }` substitutes the payload `T` to `string |
    /// number` — a node lowered in the DERIVING SFC — so the payload scope is the
    /// SFC, not the base file the generic signature was declared in.
    ///
    /// Falls back, in order, to: the SIGNATURE node's own scope (for a payload
    /// param whose node is scope-less — a primitive / composed type — on a
    /// non-generic imported signature, this keeps the scope at the signature's
    /// declaration file); the signature's declaration spans; the SFC owner.
    fn signature_expr_scope(
        &self,
        host: &VerterHost,
        sig: &crate::typeinfo::surface::TypeInfoSurfaceSignature,
    ) -> TypeExprScope {
        let graph = host.project_type_store().semantic_graph();
        let file_scope = |node: crate::semantic_query::SemanticNodeId| {
            graph
                .node_scope(node)
                .and_then(|scope| scope.canonical_file())
                .map(|canonical| TypeExprScope::new(canonical.as_ref()))
        };
        // First PAYLOAD parameter (index 1 — after the leading event-name param)
        // of the signature node's `Function` data; its type-node scope is the
        // payload's lowering file (substituted in the deriving SFC for a generic
        // signature).
        let payload_param_scope = match graph.node_data(sig.node).as_deref() {
            Some(crate::semantic_query::SemanticNodeData::Function { params, .. }) => {
                params.get(1).and_then(|param| file_scope(param.ty))
            }
            _ => None,
        };
        payload_param_scope
            .or_else(|| file_scope(sig.node))
            .or_else(|| {
                sig.signature_span
                    .as_ref()
                    .or(sig.return_type_span.as_ref())
                    .or_else(|| sig.parameter_spans.iter().flatten().next())
                    .map(|cspan| TypeExprScope::new(cspan.file.as_ref()))
            })
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
    /// **`withDefaults` returns `None` here — by design.** The analyzer routes
    /// `withDefaults(defineProps<Props>(), { … })` as TWO macros: a `DefineProps`
    /// macro carrying the `Props` type argument AND an OUTER `WithDefaults`
    /// macro that has NO type argument (the type parameter lives on the inner
    /// `defineProps`, never on the `withDefaults` call — `withDefaults<…>(…)` is
    /// not valid Vue). The outer macro is therefore `is_type_based == false` and
    /// falls out at the `is_type_based` guard below. The props surface comes
    /// from the SEPARATELY-routed inner `DefineProps` macro, exactly as the
    /// eager rail resolves it (the `WithDefaults` macro's `resolved_local_types`
    /// is empty, so the eager `cold_resolver` macro loop projects nothing for
    /// it either). The defaults the `withDefaults` call supplies flip
    /// `required` / `has_default` DOWNSTREAM at the component-meta PropAnalysis
    /// layer, not on this surface.
    ///
    /// **Provenance:** the props macro (`DefineProps`) lowers its type argument
    /// under [`ProjectionReductionContext::published_macro_type_arg_body`] so
    /// the type-argument's OWN-body members surface with
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

        // Provenance per macro axis. Props request the macro-T own-body
        // provenance on the terminal surface synthesis so the author-declared
        // members are flagged; emits / slots are structural
        // (`declared_in_macro_type_arg` is a props-axis concern). The terminal
        // `MacroTypeArgOwnBody` synthesis restamps `declared_in_macro_type_arg =
        // true` for EXACTLY the declaration's own-body direct members — it reads
        // the prepared decl's `member_index`, which is populated from direct
        // Object members only and SKIPS heritage `extends` `Ref` arms
        // (`build.rs::overlay_macro_type_arg_own_body`). Heritage-reached members
        // are NOT in `member_index`, so they are left at the structural `false`
        // the empty-path Shallow body lowering assigned. The surface's
        // `merge_role` is independently baked per arm (`Heritage` for
        // `extends`-reached members, `OwnBody` for the declaration's own body).
        // See `props_from_typeinfo_surface`.
        // Only `DefineProps` reaches here as a props macro: `WithDefaults` is
        // never `is_type_based` (it bailed at the guard above) and `DefineModel`
        // returned its empty surface above. `DefineProps` requests the macro-T
        // own-body provenance; emits / slots are structural.
        let terminal_context = match request.macro_kind {
            AnalyzedMacroKind::DefineProps => {
                ProjectionReductionContext::published_macro_type_arg_body(ProjectionMode::Shallow)
            }
            _ => ProjectionReductionContext::published(ProjectionMode::Shallow),
        };

        let store_view = self.resolver_store_view();
        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx = crate::resolver_core::HostResolverContext::new(self, &store_view, overlay);
        let dispatch = ProjectSemanticDispatch::new(&host_ctx);

        // Lower the macro type argument in the SFC scope. `Navigate` /
        // structural-transit lowering keeps member values shallow; the
        // empty-path `Shallow` projection then synthesises the one-level surface
        // under `terminal_context`.
        let base = dispatch.lower_type_expr_in_scope_with_context(
            request.owner_canonical.as_ref(),
            type_arg,
            ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
        )?;

        let surface =
            self.project_shallow_surface_from_base(&host_ctx, &dispatch, base, terminal_context)?;

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
        // Load the CURRENT `IndexedReady` BEFORE touching the cache. The
        // request's `root_identity` (a `whole_hash` hint) and `macro_kind` are
        // caller-supplied and may be STALE (a `whole_hash` captured before an
        // edit) or WRONG (a kind that disagrees with the snapshot's macro at
        // this index). Deriving both from the authoritative snapshot here means
        // a stale `root_identity` can never read an old entry (the live
        // `whole_hash` keys a fresh slot) and a wrong `macro_kind` can never
        // read or poison the sibling kind's entry (the derived kind keys the
        // slot and drives the normalizer dispatch).
        let Some(indexed) = self.ensure_indexed_ready(request.owner_canonical.as_ref()) else {
            // SFC not loaded — no surface, no cache entry. Returning the default
            // bundle WITHOUT publishing (we have no validated key) keeps the
            // cache free of entries keyed on an unvalidated identity.
            return Arc::new(VueMacroDtos::default());
        };
        let Some(mac) = indexed.snapshot.macros.get(request.macro_index) else {
            return Arc::new(VueMacroDtos::default());
        };
        let whole_hash = indexed.whole_hash;
        let macro_kind = mac.kind;

        let key = VueMacroDtoKey::new(
            Arc::clone(&request.owner_canonical),
            whole_hash,
            request.macro_index,
            macro_kind,
            request.level,
        );
        // Warm read: the content-addressed key covers the SFC's OWN content,
        // but the resolved DTOs read CROSS-FILE carrier types. Validate the
        // recorded fact signature + project generation against the live view
        // so a carrier edit (which leaves the SFC's `whole_hash` unchanged)
        // invalidates the entry lazily.
        let store_view = self.resolver_store_view();
        let generation = self.project_type_store().current_project_generation();
        if let Some(cached) =
            self.vue_shallow_metadata_store()
                .get_with_view(&key, &store_view, generation)
        {
            // Bubble the cached entry's cross-file carrier fact signature into
            // any active outer fact tracer. An outer component-meta cold trace
            // (e.g. `component_meta_resolved_macros` consuming these DTOs)
            // inherits the DTO's carrier facts on this warm hit; without the
            // bubble a prewarmed DTO would under-key the outer cache entry (a
            // carrier edit that invalidates this DTO entry must also invalidate
            // the component-meta entry that read it). Cold misses bubble
            // automatically: the `install_fact_tracer` scope below nests under
            // the outer tracer and `observe_fan_out` reaches every active cell.
            cached.read_set_signature.bubble_via_tls();
            return std::sync::Arc::clone(&cached.dtos);
        }

        // Resolve the surface through a request carrying the VALIDATED identity
        // (live `whole_hash`) and the AUTHORITATIVE kind, so the surface
        // resolution + normalizer dispatch never trust the caller's hint. The
        // whole cold resolution runs under an installed fact tracer so the
        // CROSS-FILE carrier facts it reads are captured into the entry's
        // `ReadSetSignature` (the warm-read invalidation rail above).
        let validated_request = VueMacroSurfaceRequest {
            owner_canonical: Arc::clone(&request.owner_canonical),
            macro_index: request.macro_index,
            macro_kind,
            root_identity: whole_hash,
            level: request.level,
        };
        let (dtos, finalise) = crate::fact_signature_helpers::install_fact_tracer(self, || {
            match self.resolve_vue_macro_surface(&validated_request) {
                Some(macro_surface) => match macro_kind {
                    AnalyzedMacroKind::DefineProps | AnalyzedMacroKind::DefineModel => {
                        VueMacroDtos {
                            props: props_from_typeinfo_surface(self, &macro_surface),
                            ..VueMacroDtos::default()
                        }
                    }
                    AnalyzedMacroKind::DefineEmits => VueMacroDtos {
                        emits: emits_from_typeinfo_surface(self, &macro_surface),
                        ..VueMacroDtos::default()
                    },
                    AnalyzedMacroKind::DefineSlots => VueMacroDtos {
                        slots: slots_from_typeinfo_surface(self, &macro_surface),
                        ..VueMacroDtos::default()
                    },
                    // `WithDefaults` is not a props-surface source on this path:
                    // the outer `withDefaults` macro carries no type argument (it
                    // is not `is_type_based`), so `resolve_vue_macro_surface`
                    // returns `None` for it and this arm is unreachable. The
                    // props come from the SEPARATELY-routed inner `DefineProps`
                    // macro. Options / expose are separate subsystems. None of
                    // these contribute a DTO bundle.
                    AnalyzedMacroKind::WithDefaults
                    | AnalyzedMacroKind::DefineOptions
                    | AnalyzedMacroKind::DefineExpose => VueMacroDtos::default(),
                },
                None => VueMacroDtos::default(),
            }
        });

        match finalise {
            crate::resolver_core::FactReadSetFinalise::Ok(facts) => {
                let entry = crate::typeinfo::adapters::vue::store::VueMacroDtosEntry {
                    dtos: std::sync::Arc::new(dtos),
                    read_set_signature: crate::fact_signature_helpers::ReadSetSignature::new(facts),
                    validated_at_generation: generation,
                };
                std::sync::Arc::clone(&self.vue_shallow_metadata_store().insert(key, entry).dtos)
            }
            // Tracer overflowed: the DTOs are valid but cannot be admitted
            // safely (the observation set was truncated, so warm-read
            // validation could falsely pass against a changed carrier). Return
            // the freshly-computed bundle WITHOUT caching — a repeat request
            // recomputes, never serves an under-validated entry.
            crate::resolver_core::FactReadSetFinalise::Overflow => std::sync::Arc::new(dtos),
        }
    }

    /// Navigate a `TypeExpr` to its one-level object [`TypeInfoSurface`] through
    /// the SHARED resolver, lowering it in `scope_canonical` then projecting the
    /// empty-path `Shallow` surface — the SAME machinery
    /// [`Self::resolve_shallow_surface_for`] uses for a named declaration.
    ///
    /// Used by the slot-binding extractor to resolve a slot's first-parameter
    /// type (`Pick<RowApi, 'name'>` / a named alias / a parenthesized form) to
    /// the binding object WITHOUT a nominal shape-sniff: `Pick` is navigated,
    /// `Parenthesized` is unwrapped, and an alias `Ref` is resolved by the one
    /// shared resolver rather than a per-utility special case. Returns `None`
    /// when the scope file is not loaded or the type does not project to an
    /// object surface (a primitive / union first param has no binding object).
    #[must_use]
    pub(crate) fn navigate_param_to_object_surface(
        &self,
        scope_canonical: &str,
        param_ty: &TypeExpr,
    ) -> Option<TypeInfoSurface> {
        let store_view = self.resolver_store_view();
        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx = crate::resolver_core::HostResolverContext::new(self, &store_view, overlay);
        let dispatch = ProjectSemanticDispatch::new(&host_ctx);

        // Lower the parameter type in its scope under structural-transit
        // Navigate (member values stay shallow); the empty-path Shallow
        // projection then synthesises the one-level object surface.
        let base = dispatch.lower_type_expr_in_scope_with_context(
            scope_canonical,
            param_ty,
            ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
        )?;
        self.project_shallow_surface_from_base(
            &host_ctx,
            &dispatch,
            base,
            ProjectionReductionContext::published(ProjectionMode::Shallow),
        )
    }
}

/// Slice a member's leading-JSDoc DESCRIPTION + TAG spans into owned text for
/// the published DTO. The spans are already located on the surface (by
/// `with_member_jsdoc_spans`); this reads the declaring file's cache-owned
/// source and slices — it does NOT re-locate the comment block and does NOT
/// take the lazy `member_display_jsdoc` name-search path.
///
/// Returns `(None, empty)` when the member carries no JSDoc spans or the
/// declaring file's source is unavailable.
/// Slice a [`CanonicalSpan`]'s byte range out of its file's cache-owned source
/// (`IndexedReady.eval_source`). `None` when the file is not loaded or the byte
/// range is out of bounds (a stale / synthetic span). This is the single
/// source-slicing primitive the normalizers use to materialize display text
/// from a span at the consumer boundary — it does NOT re-resolve or re-parse.
fn slice_canonical_span(host: &VerterHost, cspan: &CanonicalSpan) -> Option<String> {
    let indexed = host.ensure_indexed_ready(cspan.file.as_ref())?;
    let source = Arc::clone(&indexed.eval_source);
    let start = cspan.span.start as usize;
    let end = cspan.span.end as usize;
    source.get(start..end).map(|s| s.to_string())
}

/// Normalize a multi-line JSDoc description/tag body sliced from a span.
///
/// A description/tag span is a contiguous `[start, end)` region whose FIRST line
/// already had its leading `/**`-decoration stripped (the span starts at the
/// content), but whose CONTINUATION lines still carry the `   * ` JSDoc
/// decoration verbatim (the span is contiguous source text). The published
/// `description` is DISPLAY text, not comment syntax, so strip each
/// continuation line's leading whitespace + optional single `*` decoration —
/// matching `verter_semantic::analysis::jsdoc`'s per-line stripping — and rejoin
/// with `\n`. A single-line body is returned trimmed.
pub(crate) fn normalize_jsdoc_body(raw: &str) -> String {
    let mut lines = raw.lines();
    let mut out = String::new();
    if let Some(first) = lines.next() {
        out.push_str(first.trim_end());
    }
    for line in lines {
        out.push('\n');
        // Strip leading whitespace, then a single `*` decoration, then the
        // whitespace after it.
        let trimmed = line.trim_start();
        let stripped = trimmed
            .strip_prefix('*')
            .map(|rest| rest.trim_start())
            .unwrap_or(trimmed);
        out.push_str(stripped.trim_end());
    }
    out.trim().to_string()
}

/// Slice a leading-JSDoc description span + tag spans into the published
/// `(description, tags)` display pair. Shared by the member path
/// ([`member_jsdoc_from_spans`]) and the call-signature emit path
/// ([`signature_jsdoc_from_spans`]) — both anchor JSDoc on the typeinfo
/// surface's spans, never a reparse.
fn jsdoc_from_spans(
    host: &VerterHost,
    description_span: Option<&CanonicalSpan>,
    tag_spans: &[crate::typeinfo::surface::JsdocTagSpan],
) -> (Option<String>, Vec<JsdocTag>) {
    let slice = |cspan: &CanonicalSpan| -> Option<String> { slice_canonical_span(host, cspan) };

    let description = description_span
        .and_then(&slice)
        .map(|text| normalize_jsdoc_body(&text))
        .filter(|text| !text.is_empty());

    let tags: Vec<JsdocTag> = tag_spans
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
                .map(|t| normalize_jsdoc_body(&t))
                .filter(|t| !t.is_empty());
            Some(JsdocTag { name, text })
        })
        .collect();

    (description, tags)
}

fn member_jsdoc_from_spans(
    host: &VerterHost,
    member: &TypeInfoSurfaceMember,
) -> (Option<String>, Vec<JsdocTag>) {
    jsdoc_from_spans(
        host,
        member.jsdoc_description_span.as_ref(),
        &member.jsdoc_tag_spans,
    )
}

/// Slice a call/construct signature's leading-JSDoc into `(description, tags)`.
/// A call-signature emit (`(e: 'change', v: T): void`) documents the event via
/// the JSDoc on the signature itself — extracted here from the signature's
/// typeinfo JSDoc spans (symmetric with [`member_jsdoc_from_spans`]).
fn signature_jsdoc_from_spans(
    host: &VerterHost,
    sig: &crate::typeinfo::surface::TypeInfoSurfaceSignature,
) -> (Option<String>, Vec<JsdocTag>) {
    jsdoc_from_spans(host, sig.jsdoc_description_span.as_ref(), &sig.jsdoc_tag_spans)
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
/// `TypeExpr` scoped to its VALUE-NODE file (see
/// [`VueMacroSurface::member_expr_scope`]), the display `type_annotation`
/// rendered from that typed form, and JSDoc sliced from the surface spans.
/// Own-body-vs-heritage ordering + shadowing + union-common
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
                .map(|_| macro_surface.member_expr_scope(host, member));
            let type_annotation = type_expr.as_ref().and_then(render_type_expr_display);
            let (description, tags) = member_jsdoc_from_spans(host, member);
            // `declared_in_macro_type_arg`: a member belongs to the macro-T own
            // body iff it is NOT heritage-reached. The terminal
            // `MacroTypeArgOwnBody` synthesis already stamps this correctly — it
            // restamps `true` ONLY for the declaration's own-body
            // `member_index` members and leaves heritage-reached members at
            // `false` (`build.rs::overlay_macro_type_arg_own_body` skips
            // `extends` arms). So `member.declared_in_macro_type_arg` is already
            // authoritative. The `&& merge_role != Heritage` conjunct is
            // REDUNDANT defense-in-depth: a member can only carry
            // `declared_in_macro_type_arg == true` if it is an own-body
            // `member_index` member, which is never `merge_role == Heritage`.
            // It is kept as a belt-and-braces cross-check of the two
            // independently-baked provenance facts (the restamped flag and the
            // merge role), not because the surface over-stamps heritage members.
            let declared_in_macro_type_arg = member.declared_in_macro_type_arg
                && member.origin.merge_role != crate::semantic_query::MemberMergeRole::Heritage;
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
                declared_in_macro_type_arg,
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
///    the typed `payload_expr` is the call-signature function with the leading
///    event-name parameter STRIPPED (`(e: 'change', v: number) => void` → event
///    `change`, payload `(v: number) => void`). The event name is NEVER read
///    from `keyof` (which would surface numeric tuple indices). The display
///    `payload_type` (→ `rawType`, no consumer parses it) is a CONSISTENT
///    source-span slice of the call signature as written (local + cross-file
///    alike); `None` for a synthetic signature with no span.
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
        // Payload = the call signature's REMAINING parameters (after the leading
        // event-name parameter) as a TUPLE -- the Vue emit payload shape (the
        // args passed to `emit('name', ...)`). `(e: 'change', payload: T) =>
        // void` yields event `change` with payload tuple `[payload: T]`. This
        // matches the eager OXC rail's `AnalyzedEmitField.payload_expr` (a
        // `TypeExpr::Tuple`, NOT the whole call-signature function) so the
        // downstream projector publishes an identical `event.payload`. Each
        // surviving parameter maps to a labelled tuple element preserving its
        // name / optional / rest.
        let payload_tuple = TypeExpr::Tuple {
            elements: func
                .parameters
                .iter()
                .skip(1)
                .map(|param| verter_type_expr::TupleElement {
                    label: param.name.clone(),
                    ty: param.ty.clone(),
                    optional: param.optional,
                    rest: param.rest,
                })
                .collect(),
            readonly: false,
        };
        // Scope the payload to the call signature's DECLARATION-origin file
        // (derived from its spans) so an inherited cross-file emit signature's
        // payload `Ref`s resolve in the base file, matching the eager rail's
        // per-signature `member_expr_scope`. Falls back to the SFC owner for a
        // signature written in the SFC's own defineEmits type argument.
        let payload_scope = macro_surface.signature_expr_scope(host, sig);
        // `payload_type` (→ `rawType`) is DISPLAY-ONLY — no consumer parses it
        // (the typed `payload_expr` carries the semantics). Render it as a
        // CONSISTENT source-span slice of the call signature as written (the
        // payload function's span on the surface), for both local and cross-file
        // signatures — `render_type_expr_display` returns `None` for a function
        // and would diverge per-shape. `None` when the signature carries no span
        // (a synthetic / composed signature).
        let payload_type = sig
            .signature_span
            .as_ref()
            .and_then(|cspan| slice_canonical_span(host, cspan))
            .map(|text| text.trim().trim_end_matches(';').trim_end().to_string())
            .filter(|text| !text.is_empty());
        // The event's JSDoc rides on the call signature itself (the leading
        // `/** */` block documenting `(e: 'change', …): void`), sliced from the
        // signature's typeinfo JSDoc spans — symmetric with the property-style
        // fallback's `member_jsdoc_from_spans`. A union of event-name literals on
        // ONE signature shares that signature's JSDoc across each event it names.
        let (description, tags) = signature_jsdoc_from_spans(host, sig);
        let mut push_event = |name: String| {
            emits.push(AnalyzedEmitField {
                name,
                span: verter_span::Span::default(),
                payload_type: payload_type.clone(),
                payload_expr: Some(payload_tuple.clone()),
                payload_expr_scope: Some(payload_scope.clone()),
                description: description.clone(),
                tags: tags.clone(),
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
                .map(|_| macro_surface.member_expr_scope(host, member));
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

/// Extract a slot callable's first-parameter type + return type from a slot
/// member value, handling an INTERSECTION of function types.
///
/// A slot typed via an intersection of interfaces
/// (`defineSlots<SlotA & SlotB>()`) has its `default` member resolve to
/// `SlotA['default'] & SlotB['default']` — an `Intersection` of two function
/// types (the TS-correct meaning of indexing an intersection), NOT a single
/// pre-merged `Function`. Returns:
///
/// - `Function(f)` → `f`'s first-param type + return type directly.
/// - `Intersection(arms)` where EVERY resolvable arm is a function → the
///   INTERSECTION of the arms' first-param types (so `{ value?: string } &
///   { value: string }` flows into [`binding_fields_from_param_ty`], whose
///   resolver-navigation merges it required-wins) plus the intersection of the
///   arms' return types. A non-function arm makes the member not slot-like.
/// - Anything else → `None` (the member is not a slot).
fn slot_callable_param_and_return(
    value: &TypeExpr,
) -> Option<(Option<TypeExpr>, Option<TypeExpr>)> {
    match value {
        TypeExpr::Function(func) => Some((
            func.parameters.first().map(|p| p.ty.clone()),
            func.return_type.as_ref().map(|rt| (**rt).clone()),
        )),
        TypeExpr::Intersection(arms) => {
            let mut first_params: Vec<TypeExpr> = Vec::new();
            let mut returns: Vec<TypeExpr> = Vec::new();
            for arm in arms.iter() {
                let TypeExpr::Function(func) = arm else {
                    // A non-function arm means the member is not purely
                    // slot-callable; fall out (not a slot).
                    return None;
                };
                if let Some(p) = func.parameters.first() {
                    first_params.push(p.ty.clone());
                }
                if let Some(rt) = func.return_type.as_ref() {
                    returns.push((**rt).clone());
                }
            }
            if first_params.is_empty() && returns.is_empty() {
                return None;
            }
            let first_param = match first_params.len() {
                0 => None,
                1 => Some(first_params.into_iter().next().unwrap()),
                _ => Some(TypeExpr::Intersection(std::sync::Arc::from(
                    first_params.into_boxed_slice(),
                ))),
            };
            let return_ty = match returns.len() {
                0 => None,
                1 => Some(returns.into_iter().next().unwrap()),
                _ => Some(TypeExpr::Intersection(std::sync::Arc::from(
                    returns.into_boxed_slice(),
                ))),
            };
            Some((first_param, return_ty))
        }
        _ => None,
    }
}

/// Normalize a `.vue` slots macro surface into the published
/// [`AnalyzedSlotField`] set.
///
/// Reproduces `ImportedMacroSurface::slot_members` over the typeinfo surface:
/// keep FUNCTION-LIKE members only (the value raises to a `TypeExpr::Function`;
/// non-function members are filtered); the slot's `bindings` come from
/// resolving the function's first-parameter type to its object surface (a
/// literal object, a `Pick<…>`, or a named alias — see
/// [`binding_fields_from_param_ty`]); the `return_expr` / `return_type` come
/// from the function's return type. Bindings + return are scoped to the slot
/// member's VALUE-NODE file (see [`VueMacroSurface::member_expr_scope`]).
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
            // A slot member is function-like: a single `Function`, or an
            // `Intersection` of functions (`(SlotA & SlotB)['default']`). A
            // non-callable member is not a slot.
            let (first_param, return_expr) = slot_callable_param_and_return(&value)?;
            let scope = macro_surface.member_expr_scope(host, member);
            let bindings = first_param
                .as_ref()
                .map(|param_ty| binding_fields_from_param_ty(host, param_ty, &scope))
                .unwrap_or_default();
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
/// type. Each member of the parameter's OBJECT surface becomes one
/// [`AnalyzedSlotFieldBinding`] carrying that member's value `TypeExpr` as
/// `binding_expr`.
///
/// The first parameter is the slot-props object. It can be written several
/// ways, all of which the LOCAL eager rail
/// (`surface_projector::bindings_from_first_param_ty`) accepts: a literal
/// object (`(props: { item: string })`), a `Pick<T, 'k'>` over a named type
/// (`(props: Pick<RowApi, 'name'>)`), or a parenthesized form. To handle all of
/// them WITHOUT a nominal shape-sniff (no `name == "Pick"` matching, no text
/// splitting), the binding object is obtained by RESOLVING the first-parameter
/// type through the SHARED resolver:
///
/// - A literal [`TypeExpr::Object`] is read directly (no resolution needed — it
///   is already the binding object). This is a STRUCTURAL match on the typed
///   IR, not a nominal sniff.
/// - Any other shape (`Pick<…>` / `Omit<…>` / a `Ref` to a named alias /
///   `Parenthesized`) is lowered in the slot member's value-node scope and
///   projected to its one-level object surface
///   ([`VerterHost::navigate_param_to_object_surface`]); each surface member
///   becomes a binding. The shared resolver navigates `Pick` / unwraps
///   `Parenthesized` / resolves the alias — there is no per-utility special
///   case here.
///
/// A first parameter that does not resolve to an object surface yields no
/// bindings — matching the eager rail's non-object outcome.
fn binding_fields_from_param_ty(
    host: &VerterHost,
    param_ty: &TypeExpr,
    scope: &TypeExprScope,
) -> Vec<AnalyzedSlotFieldBinding> {
    // Literal object: read its properties directly (structural typed-IR match).
    if let TypeExpr::Object(obj) = param_ty {
        return obj
            .properties
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
            .collect();
    }

    // Non-object first param (`Pick<…>` / alias `Ref` / `Parenthesized`):
    // navigate it through the shared resolver to its object surface and read
    // the resolved members. Each member's `binding_expr` is its raised value
    // type, scoped to that value's own file (so a `Pick<RowApi,'k'>` whose
    // picked member is a cross-file `Ref` resolves in the right file).
    let Some(surface) = host.navigate_param_to_object_surface(scope.as_str(), param_ty) else {
        return Vec::new();
    };
    surface
        .members
        .iter()
        .map(|member| {
            let binding_expr = raise_member_value(host, member);
            let binding_expr_scope = binding_expr
                .as_ref()
                .map(|_| macro_member_value_scope(host, member, scope));
            let type_annotation = binding_expr.as_ref().and_then(render_type_expr_display);
            AnalyzedSlotFieldBinding {
                name: member.name.as_ref().to_string(),
                type_annotation,
                binding_expr,
                binding_expr_scope,
                span: verter_span::Span::default(),
            }
        })
        .collect()
}

/// The [`TypeExprScope`] a navigated binding member's `binding_expr` binds to —
/// its value-node scope (matching [`VueMacroSurface::member_expr_scope`]),
/// falling back to the slot's scope when the member's value node is
/// structural / scope-less.
fn macro_member_value_scope(
    host: &VerterHost,
    member: &TypeInfoSurfaceMember,
    fallback: &TypeExprScope,
) -> TypeExprScope {
    host.project_type_store()
        .semantic_graph()
        .node_scope(member.value)
        .and_then(|scope| scope.canonical_file())
        .map(|canonical| TypeExprScope::new(canonical.as_ref()))
        .or_else(|| {
            member
                .origin
                .canonical_file
                .as_ref()
                .map(|canonical| TypeExprScope::new(canonical.as_ref()))
        })
        .unwrap_or_else(|| fallback.clone())
}
