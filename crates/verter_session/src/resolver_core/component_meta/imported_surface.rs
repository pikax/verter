//! Typed-IR bridge for imported macro surfaces.
//!
//! # Purpose
//!
//! The [`ImportedMacroSurface`] bridge wraps an imported
//! declaration identity and exposes lazy projection accessors
//! that compose the existing
//! [`SemanticQueryKey::ResolveDecl`] +
//! [`SemanticQueryKey::ProjectPath`] dispatch vocabulary onto
//! that declaration. The bridge does not introduce a new query
//! key; it does not store semantic-graph state; it dispatches
//! through [`ResolverContext::dispatch`] at the call site so the
//! request's audit / dep-signature / cache-suppress machinery
//! observes the work.
//!
//! # Why a separate bridge module
//!
//! The bridge is the deliberate seam between the OXC-driven
//! resolution rail (where imported targets currently materialize
//! a full [`ResolvedElements`] via an eager `resolve_type` walk)
//! and the dispatch-driven typed-IR rail (where a single member
//! can be projected without enumerating all siblings). The
//! bridge:
//!
//! - keeps the typed-IR projection API in `verter_session` only —
//!   `verter_semantic`, `verter_protocol`, `verter_ffi`, and the
//!   JS compat layers MUST NOT reference [`ImportedMacroSurface`].
//!   Architecture guards in `tests/architecture_guards.rs` pin
//!   this.
//! - exposes only **explicit-dispatch** accessors. Every
//!   dispatching public method on [`ImportedMacroSurface`] takes
//!   `&dyn ResolverContext` so the call site cannot accidentally
//!   trigger a lazy read through `&self` alone. This satisfies
//!   R25 (cold-path-only tracer) and R31 (explicit policy
//!   identity) — lazy work must be visible at the call site, not
//!   hidden behind a `Drop` or a `&self`-only accessor that
//!   opens a TLS-backed cache read.
//! - reuses the existing [`SemanticQueryKey::ResolveDecl`] +
//!   [`SemanticQueryKey::ProjectPath`] composition vocabulary
//!   rather than introducing a new query variant.
//!   [`SemanticQueryKey::ResolveMacroPayload`] is owner-macro
//!   sidecar logic keyed on `(owner, macro_index)` and is
//!   deliberately NOT used here — imported targets are resolved
//!   declarations, not owner-side macro payloads.
//!
//! # Composition vocabulary
//!
//! Given an imported declaration identity `(canonical_id,
//! type_name, whole_hash)`:
//!
//! - [`ImportedMacroSurface::resolve_root`] dispatches a
//!   [`SemanticQueryKey::ResolveDecl`] keyed on
//!   `ScopeId { canonical_id, local_scope: None }` + `type_name`
//!   and returns the resulting [`SemanticNodeId`].
//! - [`ImportedMacroSurface::project_named_member`] composes
//!   [`SemanticQueryKey::ResolveDecl`] with a single-hop
//!   [`SemanticQueryKey::ProjectPath`] whose path is
//!   `[PathSegment::Member(name)]`. The terminal hop runs in the
//!   caller-supplied [`ProjectionMode`].
//!
//! Both helpers run through [`ResolverContext::dispatch`] and
//! therefore inherit the existing dep-signature merge / cache
//! identity / completion-fence guarantees of
//! [`crate::project_semantic_dispatch::ProjectSemanticDispatch`].
//! No private side channel.
//!
//! # Why explicit `&dyn ResolverContext`
//!
//! Lazy access requires `ResolverContext` / dispatch,
//! dep-signature merging, diagnostics, cache-suppress
//! propagation, and projection mode. Hidden lazy reads behind
//! `&self` would violate R25/R31.
//!
//! Every dispatching public accessor therefore takes an explicit
//! `&dyn ResolverContext` parameter so:
//!
//! 1. The dispatch chain (and its dep-signature merge into the
//!    caller's request-scoped accumulator) is observable at the
//!    call site.
//! 2. The projection mode is supplied explicitly — the bridge
//!    has no implicit "default" mode that would make a
//!    `Shallow` reader silently get an `Expanded` projection.
//! 3. The bridge holds no resolver state internally —
//!    [`ImportedMacroSurface`] is pure declaration identity and
//!    is safe to `Clone`, store, and pass across thread
//!    boundaries.
//!
//! # Macro-surface accessor (the canonical path)
//!
//! The [`ResolvedMacroSurface::LazyImported`] arm's
//! `prop_members` / `emit_members` / `slot_members` read the
//! imported declaration's one-level surface through
//! [`crate::project_semantic_dispatch::ProjectSemanticDispatch::surface_view_from_base_node`],
//! carrying the macro-surface
//! [`crate::semantic_query::SurfaceProvenanceContext`] (props enter
//! under `MacroTypeArgOwnBody` so own-body members surface with
//! `declared_in_macro_type_arg = true`; emits/slots are structural).
//! This is the macro-aware shared resolver — the imported
//! declaration's own-body-vs-heritage provenance is decided by the
//! per-arm declaration-body lowering inside `build_instantiate`, not
//! by a private walk. The earlier `ResolveDecl`-only surface read
//! lost the provenance bit (every member reported `false`); reading
//! through the provenance-carrying surface reader restores it.
//!
//! # Current behaviour
//!
//! The bridge is pure typed-IR infrastructure. The existing eager
//! resolution rail still produces
//! [`crate::resolver_core::ResolvedImportedMacroSurface`] for every
//! imported macro target — production has NOT flipped onto the
//! [`ResolvedMacroSurface::LazyImported`] arm; the canonical path is
//! proven field-for-field equivalent to the eager arm (including
//! `declared_in_macro_type_arg`) by the equivalence discriminators in
//! `tests/canonical_macro_surface_equivalence.rs` and
//! `tests/stage2b1_macro_authority_equivalence.rs`. The
//! [`AuditEvent::ImportedMacroSurfaceProjection`] counter fires once
//! per public bridge accessor entry.
//!
//! The auxiliary identity accessors
//! ([`ImportedMacroSurface::resolve_root`],
//! [`ImportedMacroSurface::project_named_member`],
//! [`ImportedMacroSurface::enumerate_member_names`]) remain for
//! test-harness coverage of the underlying `ResolveDecl` /
//! `ProjectPath` / `key_names_from_base_node` composition; they are
//! not on the production macro-surface path.

use std::sync::Arc;

use verter_semantic::analysis::types::{
    AnalyzedEmitField, AnalyzedMacroKind, AnalyzedPropField, AnalyzedSlotField,
    AnalyzedSlotFieldBinding,
};
use verter_type_expr::{LiteralValue, TypeExpr, TypeExprScope};

use crate::resolver_core::{
    ResolvedJsdocBlock, ResolvedNativeProp, ResolvedTypeDeclaration, ResolverContext,
};
use crate::semantic_query::{
    HashValue, PathSegment, ProjectionMode, QueryResult, ResolveDeclKey, ScopeId, SemanticNodeId,
    SemanticQueryApi, SemanticQueryKey,
};

/// Identity of an imported macro target's declaration.
///
/// This is the lazy projection base. It captures the
/// `(canonical_id, type_name, whole_hash)` triple needed to dispatch
/// a [`SemanticQueryKey::ResolveDecl`] for the imported declaration
/// without re-walking the import graph or re-resolving the import
/// route. Producers populate the identity from the
/// imported-registry lookup once and then hand the bridge to
/// downstream consumers that project specific members on demand.
///
/// The triple is the smallest set of fields that:
///
/// - identifies the file the declaration lives in
///   ([`Self::canonical_id`]),
/// - identifies the declaration within that file
///   ([`Self::type_name`] — the bare exported name, e.g. `"Props"`
///   or `"ChatMessage"`),
/// - pins the content version so a stale identity does not address
///   a freshly-edited file ([`Self::whole_hash`]).
///
/// Equality / hashing is structural, so two callers that derive the
/// same triple share one cache entry under the
/// `SemanticQueryKey::ResolveDecl` memo.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImportedDeclarationIdentity {
    /// Canonical file id where the imported declaration lives.
    pub canonical_id: Arc<str>,
    /// Bare exported type name (e.g. `"Props"`, `"ChatMessage"`).
    /// This is what callers pass to
    /// [`SemanticQueryKey::ResolveDecl`]'s `name` slot.
    pub type_name: Arc<str>,
    /// Content-hash of the declaring file at identity construction
    /// time. Carried so consumers that revalidate the bridge
    /// against the live store view before projecting can compare
    /// the captured hash with the current authoritative hash.
    /// The bridge itself does not enforce revalidation — the
    /// underlying `ResolveDecl` dispatch already routes through
    /// the content-pinned `FileArtifactStore` lookup.
    pub whole_hash: HashValue,
}

impl ImportedDeclarationIdentity {
    /// Construct a new identity. Producers build this from an
    /// [`crate::resolver_core::ResolvedTypeDeclaration`] snapshot
    /// taken at imported-registry resolution time.
    #[inline]
    #[must_use]
    pub fn new(canonical_id: Arc<str>, type_name: Arc<str>, whole_hash: HashValue) -> Self {
        Self {
            canonical_id,
            type_name,
            whole_hash,
        }
    }

    /// Top-level scope for the declaration's file.
    /// [`SemanticQueryKey::ResolveDecl`] expects a [`ScopeId`] +
    /// name pair; this helper centralises the conversion so
    /// callers cannot accidentally pass a `local_scope` index.
    #[inline]
    #[must_use]
    pub fn top_level_scope(&self) -> ScopeId {
        ScopeId {
            canonical_id: Arc::clone(&self.canonical_id),
            local_scope: None,
        }
    }
}

/// Lazy typed-IR-backed surface for an imported macro target.
///
/// Construction is cheap: only the declaration identity is captured
/// — no semantic graph state, no resolver context, no in-flight
/// dispatch handle. Every projection runs through
/// [`ResolverContext::dispatch`] at the call site so the request's
/// audit / dep-signature / cache-suppress machinery observes the
/// work the bridge performs.
///
/// **Producers MUST be the imported-registry / route resolver —
/// never a fallthrough text walker.** The bridge is the typed-IR
/// equivalent of the OXC eager-resolution rail; any producer
/// outside the route-graph rail would re-introduce the bypass.
///
/// **Consumers MUST call the explicit-dispatch accessors below.**
/// Cloning the bridge and projecting twice from two threads is
/// allowed; each projection routes through dispatch independently.
#[derive(Debug, Clone)]
pub struct ImportedMacroSurface {
    identity: ImportedDeclarationIdentity,
}

impl ImportedMacroSurface {
    /// Construct a bridge for the given imported declaration
    /// identity.
    ///
    /// The function is total — it allocates no resolver state
    /// and dispatches no work. The dispatch happens at
    /// projection time when callers invoke the
    /// explicit-dispatch accessors below.
    #[inline]
    #[must_use]
    pub fn new(identity: ImportedDeclarationIdentity) -> Self {
        Self { identity }
    }

    /// Borrow the underlying declaration identity.
    ///
    /// Consumers may use this to compose a higher-level query
    /// (e.g. an `Instantiate` with explicit type arguments)
    /// without re-deriving the canonical / name / hash.
    #[inline]
    #[must_use]
    pub fn identity(&self) -> &ImportedDeclarationIdentity {
        &self.identity
    }

    /// Resolve the imported declaration to its root
    /// [`SemanticNodeId`].
    ///
    /// Composes
    /// [`SemanticQueryKey::ResolveDecl`] through
    /// [`ResolverContext::dispatch`]. Bumps the
    /// [`verter_audit::AuditEvent::ImportedMacroSurfaceProjection`]
    /// counter exactly once per call (regardless of warm/cold) so
    /// downstream consumers can confirm the bridge's fire rate
    /// against the existing eager-resolution rail's fire rate.
    ///
    /// Crate-public because `ResolverContext` is `pub(crate)` — the
    /// bridge is intra-crate infrastructure that consumers reach
    /// through other crate-public producer surfaces
    /// (component-meta cold resolver, imported registry rail).
    pub(crate) fn resolve_root(&self, ctx: &dyn ResolverContext) -> QueryResult<SemanticNodeId> {
        bump_projection_counter();
        let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: self.identity.top_level_scope(),
            name: Arc::clone(&self.identity.type_name),
        });
        ctx.dispatch().execute(key)
    }

    /// Project a single named member of the imported declaration in
    /// the supplied [`ProjectionMode`].
    ///
    /// Composes [`SemanticQueryKey::ResolveDecl`] with a single-hop
    /// [`SemanticQueryKey::ProjectPath`] whose path is
    /// `[PathSegment::Member(name)]`. The intermediate hop is the
    /// resolved declaration; the terminal hop is the named member
    /// in the caller-supplied mode.
    ///
    /// Returns the [`SemanticNodeId`] of the projected member.
    /// Callers convert to a [`verter_type_expr::TypeExpr`] via
    /// `ProjectSemanticDispatch::raise_node_to_type_expr` when the
    /// downstream payload needs structural shape.
    ///
    /// Failure semantics:
    ///
    /// - [`QueryError::Miss`](crate::semantic_query::QueryError::Miss)
    ///   — declaration could not be located in the imported file's
    ///   shallow state (typically a stale identity).
    /// - [`QueryError::DeclPlaceholder`](crate::semantic_query::QueryError::DeclPlaceholder)
    ///   — declaration exists but no member with this name is
    ///   present on the surface.
    /// - Other variants flow through unchanged from the underlying
    ///   dispatch.
    ///
    /// Crate-public because `ResolverContext` is `pub(crate)` — see
    /// the note on [`Self::resolve_root`].
    pub(crate) fn project_named_member(
        &self,
        ctx: &dyn ResolverContext,
        name: &str,
        mode: ProjectionMode,
    ) -> QueryResult<SemanticNodeId> {
        bump_projection_counter();
        // Two-hop composition. The intermediate `ResolveDecl` hop
        // shares the warm entry with `resolve_root`; the terminal
        // `ProjectPath` hop runs in the caller-supplied mode.
        let dispatch = ctx.dispatch();
        let root = match dispatch.execute(SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: self.identity.top_level_scope(),
            name: Arc::clone(&self.identity.type_name),
        })) {
            QueryResult::Value(node) => node,
            other => return other,
        };
        let path: Arc<[PathSegment]> = Arc::from([PathSegment::Member(Arc::from(name))]);
        dispatch.project_path(root, path, mode)
    }

    /// Enumerate the named members of this imported macro surface.
    ///
    /// Composition (path-precise, name-level only):
    ///
    /// 1. Dispatch [`SemanticQueryKey::ResolveDecl`] to obtain the
    ///    declaration's [`SemanticNodeId`] (the same root
    ///    [`Self::resolve_root`] returns).
    /// 2. Hand that node to the single shared `keyof`-level
    ///    enumerator
    ///    [`crate::project_semantic_dispatch::ProjectSemanticDispatch::key_names_from_base_node`].
    ///    The enumerator owns the declaration-placeholder unwrap
    ///    (instantiating the bare interface/alias body so its member
    ///    surface is readable) and the `keyof (A & B)` / `keyof (A |
    ///    B)` Intersection/Union accumulation — the bridge does NOT
    ///    re-implement either. It reads member NAMES off the surface;
    ///    it never projects a member's value type.
    ///
    /// This is the `keyof`-level surface: the result is the member
    /// name set, NOT the projected member types. Consumers that need
    /// a member's value type call [`Self::project_named_member`] per
    /// name afterwards (lazy, path-precise) or materialize the full
    /// set eagerly (the FFI / `ResolvedMacroInput` case). Either way
    /// enumeration itself issues no `ProjectPath` / `ProjectMember`
    /// dispatch — it is strictly name discovery, so a deeply-nested
    /// member value type (`wanted: Heavy`) is never walked.
    ///
    /// Reusing the dedicated enumerator (rather than a private
    /// raise + member-name walk) keeps the bridge on the one shared
    /// resolver path: any fix to `keyof` enumeration semantics
    /// benefits the bridge automatically, and the bridge cannot
    /// drift into a second member-name walker.
    ///
    /// Returns the member names as `Arc<str>` so each name round-trips
    /// directly into [`Self::project_named_member`] without a fresh
    /// allocation.
    ///
    /// Failure / empty semantics:
    ///
    /// - The root [`SemanticQueryKey::ResolveDecl`] dispatch's
    ///   [`QueryResult::Error`] / [`QueryResult::Recursive`] variants
    ///   propagate unchanged.
    /// - A resolved root with no enumerable member surface (a
    ///   primitive, a still-deferred shell, a conditional that did
    ///   not reduce, …) yields an empty `Vec`. "Surface present but
    ///   exposes no enumerable members" and "surface unresolvable"
    ///   collapse to the same empty result here; callers that must
    ///   distinguish them inspect [`Self::resolve_root`] directly.
    ///
    /// Bumps the
    /// [`verter_audit::AuditEvent::ImportedMacroSurfaceProjection`]
    /// counter exactly once per call (matching the
    /// once-per-public-accessor discipline of [`Self::resolve_root`]
    /// and [`Self::project_named_member`]). Enumeration is a bridge
    /// accessor, so it contributes to the same bridge-demand counter
    /// rather than a speculative enumeration-only footprint field
    /// (there are no production consumers yet — a finer
    /// enumerate-vs-project split is wired with the consumer
    /// migration, where the footprint surface is regenerated anyway).
    ///
    /// Crate-public because `ResolverContext` is `pub(crate)` — see
    /// the note on [`Self::resolve_root`].
    pub(crate) fn enumerate_member_names(
        &self,
        ctx: &dyn ResolverContext,
    ) -> QueryResult<Vec<Arc<str>>> {
        bump_projection_counter();
        let dispatch = ctx.dispatch();
        // Step 1: resolve the declaration root. We dispatch
        // `ResolveDecl` directly (rather than re-entering
        // `resolve_root`) so enumeration bumps the bridge-demand
        // counter exactly once, not twice.
        let root = match dispatch.execute(SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: self.identity.top_level_scope(),
            name: Arc::clone(&self.identity.type_name),
        })) {
            QueryResult::Value(node) => node,
            QueryResult::Error(err) => return QueryResult::Error(err),
            QueryResult::Recursive(node) => return QueryResult::Recursive(node),
        };
        // Step 2: enumerate member names via the single shared
        // `keyof`-level enumerator. It owns placeholder unwrap +
        // Intersection/Union accumulation and reads names only —
        // never projecting a member's value type. `None` (no
        // enumerable surface) collapses to an empty member set.
        let names = dispatch.key_names_from_base_node(root).unwrap_or_default();
        QueryResult::Value(names)
    }

    pub(crate) fn resolve_surface_view(
        &self,
        ctx: &dyn ResolverContext,
        provenance: crate::semantic_query::SurfaceProvenanceContext,
    ) -> QueryResult<crate::project_semantic_dispatch::enumerate::MacroSurfaceView> {
        bump_projection_counter();
        let dispatch = ctx.dispatch();
        // Step 1: resolve the imported declaration root (the same root
        // `resolve_root` returns).
        let root = match dispatch.execute(SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: self.identity.top_level_scope(),
            name: Arc::clone(&self.identity.type_name),
        })) {
            QueryResult::Value(node) => node,
            QueryResult::Error(err) => return QueryResult::Error(err),
            QueryResult::Recursive(node) => return QueryResult::Recursive(node),
        };
        // Step 2: read the one-level surface through the single shared
        // surface enumerator, carrying the caller's macro-surface
        // provenance (codex BINDING design). The
        // enumerator owns the DeclPlaceholder unwrap + `A & B`
        // Intersection accumulation and preserves the surface's call
        // signatures (the `defineEmits` call-signature event extractor
        // reads them) — the empty-path `Published(Shallow)` synthesis
        // would have DROPPED call signatures, so we read the surface
        // directly via the enumerator instead. When `provenance ==
        // MacroTypeArgOwnBody` the enumerator's DeclPlaceholder unwrap
        // stamps the imported declaration's OWN-body members
        // `declared_in_macro_type_arg = true` and heritage-reached
        // members stay `false`.
        let view = dispatch
            .surface_view_from_base_node(root, provenance)
            .unwrap_or_default();
        QueryResult::Value(view)
    }

    /// Raise a member value node back to a [`TypeExpr`] through the
    /// shared structural raiser. Returns `None` when the node has no
    /// raisable shape (the caller substitutes `TypeExpr::Unknown`,
    /// matching the eager rail's missing-`type_expr` fallback).
    fn raise_member_value(ctx: &dyn ResolverContext, node: SemanticNodeId) -> Option<TypeExpr> {
        ctx.dispatch().raise_node_to_type_expr(node)
    }

    /// The canonical file a member's value node was first lowered in — the
    /// member ORIGIN. Read from the value node's origin sidecar
    /// ([`SemanticGraphStore::node_scope`]). For an INHERITED member
    /// (`interface Props extends Base`) the origin is the heritage BASE's
    /// file, not the root declaration's. Returns `None` for a structural /
    /// `Global`-scoped value node (a primitive, a shared literal-union)
    /// whose origin is not a single declaration file.
    fn member_origin_canonical(
        ctx: &dyn ResolverContext,
        member_value: SemanticNodeId,
    ) -> Option<Arc<str>> {
        match ctx
            .project_type_store()
            .semantic_graph()
            .node_scope(member_value)
        {
            Some(crate::semantic_query::NodeScopeId::File { canonical_id, .. }) => {
                Some(canonical_id)
            }
            _ => None,
        }
    }

    /// The [`TypeExprScope`] for a member's paired `*_expr` field.
    ///
    /// The scope names the file whose OXC parse produced the typed
    /// expression — consumers walking nested `TypeExpr::Ref` nodes resolve
    /// them in THAT file (see [`TypeExprScope`]). For an INHERITED member the
    /// type-expr was written in the heritage BASE's file, so the scope MUST
    /// be the member's ORIGIN file ([`Self::member_origin_canonical`]), not
    /// the derived/root declaration's file: a base-file member typed as a
    /// `LocalAlias` raises to `Ref("LocalAlias")`, and the root-file scope
    /// would make later typed-IR resolution look in the WRONG file (a Miss /
    /// a cross-file mis-binding). Falls back to the surface root scope when
    /// the value node carries no single-file origin (a structural node).
    fn member_expr_scope(
        ctx: &dyn ResolverContext,
        member_value: SemanticNodeId,
        root_scope: &TypeExprScope,
    ) -> TypeExprScope {
        Self::member_origin_canonical(ctx, member_value)
            .map(|canonical| TypeExprScope::new(canonical.as_ref()))
            .unwrap_or_else(|| root_scope.clone())
    }

    /// Reattach a member's JSDoc (`description` + `tags`) from the cached
    /// parse artifact of the member's ORIGIN file — the display sidecar.
    ///
    /// The member's value node carries an origin scope
    /// ([`SemanticGraphStore::node_scope`]) recording the canonical file
    /// where the member's declaration was lowered. For an INHERITED member
    /// (`interface Props extends Base`) that scope is the heritage BASE's
    /// file, not the root declaration's — so the JSDoc follows the member
    /// ORIGIN, exactly as the brief requires.
    ///
    /// **Declaration provenance (not a file-wide first match).** A single
    /// file may declare the same property name in TWO declarations (only one
    /// of which is the heritage base an inherited member came from), and a
    /// member may be method-style (`default(props): any`). A whole-file
    /// `name:` search would attach the FIRST textual match — possibly the
    /// wrong declaration — and miss method members entirely. So we scope the
    /// JSDoc search to the DECLARING declaration's span: enumerate the origin
    /// file's local type declarations
    /// ([`AnalyzedExternalTypeSource::local_type_symbol_spans`]), keep those
    /// whose span declares `member_name` as a direct member, and — when more
    /// than one declaration declares the name — disambiguate by matching the
    /// member's VALUE NODE against each candidate declaration's same-named
    /// member value node (the cache-owned own-surface read; structural
    /// interning makes the inherited member's value node identical to the
    /// declaring declaration's). The search itself reads the file's
    /// cache-owned `IndexedReady.eval_source` (no re-resolution, no fresh
    /// parse) and accepts property- AND method-style members. This mirrors
    /// the eager rail's `enrich_projected_jsdoc`, which resolves each
    /// member's JSDoc from its declaration source by span; the lazy rail
    /// resolves by member name + declaration span because the typed IR is
    /// span-free.
    ///
    /// Returns `(None, empty)` when the origin scope is unknown, the source
    /// is unavailable, or no leading JSDoc is present — matching the eager
    /// rail's "no JSDoc" outcome.
    fn member_display_jsdoc(
        ctx: &dyn ResolverContext,
        member_value: SemanticNodeId,
        member_name: &str,
    ) -> (
        Option<String>,
        Vec<verter_semantic::analysis::types::JsdocTag>,
    ) {
        let Some(canonical_id) = Self::member_origin_canonical(ctx, member_value) else {
            return (None, Vec::new());
        };
        let Some(indexed) = ctx.ensure_indexed_ready(canonical_id.as_ref()) else {
            return (None, Vec::new());
        };
        let source = indexed.eval_source.as_ref();

        // Determine the declaring declaration's span so the JSDoc search is
        // scoped to it (not a file-wide first match). When the origin file's
        // type analysis is unavailable, fall back to the whole-file search
        // (the prior behaviour — still correct for single-declaration files).
        if let Some((start, end)) =
            Self::declaring_decl_span(ctx, source, &canonical_id, member_value, member_name)
        {
            return verter_semantic::analysis::jsdoc::extract_jsdoc_for_property_name_in_range(
                source,
                member_name,
                start,
                end,
            );
        }

        verter_semantic::analysis::jsdoc::extract_jsdoc_for_property_name(source, member_name)
    }

    /// Resolve the `(start, end)` byte span of the declaration in
    /// `canonical_id` that declares `member_name` as the member carried by
    /// `member_value`. Returns `None` when the origin file's type analysis is
    /// unavailable, or no local declaration declares the member name (e.g. it
    /// was reached from a deeper file) so the caller falls back to the
    /// whole-file search.
    ///
    /// Disambiguation when several declarations in the file declare the same
    /// member name: resolve each candidate declaration's own one-level
    /// surface through the cache-owned dispatch and keep the candidate whose
    /// same-named member's value node equals `member_value`. Structural
    /// interning guarantees an inherited member's value node is identical to
    /// the declaring declaration's member value node, so the match is exact
    /// and data-driven (no name-suffix / ordinal heuristic).
    ///
    /// The origin file's declaration index is the cache-owned
    /// `external_type_analysis` artifact (declaration names + full spans) —
    /// no re-parse, no re-resolution.
    fn declaring_decl_span(
        ctx: &dyn ResolverContext,
        source: &str,
        canonical_id: &Arc<str>,
        member_value: SemanticNodeId,
        member_name: &str,
    ) -> Option<(usize, usize)> {
        let analysis = ctx.external_type_analysis(canonical_id.as_ref())?;
        // Candidate declarations: those whose full span contains a
        // source-level declaration of `member_name` (property- or
        // method-style). A declaration with no `member_name` declaration in
        // its span cannot be the origin.
        let mut candidates: Vec<(usize, usize)> = analysis
            .local_type_symbol_spans()
            .filter_map(|(_name, span)| {
                let start = span.start as usize;
                let end = (span.end as usize).min(source.len());
                if start >= end {
                    return None;
                }
                // A declaration "declares" the member if the member-name
                // declaration site exists in its span — even when that site
                // carries no leading JSDoc (so a JSDoc-less declaring
                // declaration still anchors the scope and is not skipped in
                // favour of a later JSDoc-bearing decoy). The presence probe
                // does not require JSDoc.
                member_decl_site_in_range(source, member_name, start, end).then_some((start, end))
            })
            .collect();

        match candidates.len() {
            0 => None,
            1 => Some(candidates.remove(0)),
            _ => {
                // Ambiguous: multiple declarations declare the member name.
                // Disambiguate by the member's value node identity.
                let dispatch = ctx.dispatch();
                for &(start, end) in &candidates {
                    // The declaration name whose span starts at `start`.
                    let Some(decl_name) = analysis
                        .local_type_symbol_spans()
                        .find(|(_n, s)| s.start as usize == start)
                        .map(|(n, _s)| n.to_string())
                    else {
                        continue;
                    };
                    let root =
                        match dispatch.execute(SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                            scope: ScopeId {
                                canonical_id: Arc::clone(canonical_id),
                                local_scope: None,
                            },
                            name: Arc::from(decl_name.as_str()),
                        })) {
                            QueryResult::Value(node) => node,
                            QueryResult::Error(_) | QueryResult::Recursive(_) => continue,
                        };
                    let Some(view) = dispatch.surface_view_from_base_node(
                        root,
                        crate::semantic_query::SurfaceProvenanceContext::Structural,
                    ) else {
                        continue;
                    };
                    if view
                        .members
                        .iter()
                        .any(|m| m.name.as_ref() == member_name && m.value == member_value)
                    {
                        return Some((start, end));
                    }
                }
                // No value-node match (e.g. same-typed duplicate whose value
                // nodes structurally collapse). Fall back to the whole-file
                // search rather than guess a declaration.
                None
            }
        }
    }
}

/// Whether `source[range_start..range_end)` contains a direct declaration of
/// `member_name` (property-style `name:` / `name?:` or method-style `name(`),
/// independent of whether that site carries leading JSDoc. Used by
/// [`ImportedMacroSurface::declaring_decl_span`] to anchor a declaring
/// declaration whose member has no JSDoc, so a later JSDoc-bearing decoy
/// declaration does not win the scope.
fn member_decl_site_in_range(
    source: &str,
    member_name: &str,
    range_start: usize,
    range_end: usize,
) -> bool {
    if member_name.is_empty() {
        return false;
    }
    let bytes = source.as_bytes();
    let range_end = range_end.min(bytes.len());
    if range_start >= range_end {
        return false;
    }
    let pat = member_name.as_bytes();
    let mut search_start = range_start;
    while let Some(rel) = source.get(search_start..range_end).and_then(|window| {
        window
            .find(member_name)
            .filter(|rel| search_start + rel + pat.len() <= range_end)
    }) {
        let abs = search_start + rel;
        let after = abs + pat.len();
        let boundary_before = abs == 0 || !is_ident_continue(bytes[abs - 1]);
        let boundary_after = after >= bytes.len() || !is_ident_continue(bytes[after]);
        if boundary_before && boundary_after {
            let mut cursor = after;
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            if cursor < bytes.len() && bytes[cursor] == b'?' {
                cursor += 1;
            }
            if cursor < bytes.len() && (bytes[cursor] == b':' || bytes[cursor] == b'(') {
                return true;
            }
        }
        search_start = abs + 1;
    }
    false
}

#[inline]
fn is_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
}

/// Eager-resolved-macro newtype mirroring the existing public
/// fields on [`crate::resolver_core::ResolvedMacroMeta`].
///
/// The newtype's purpose is purely structural: it gives the
/// [`ResolvedMacroSurface::Eager`] arm a typed payload distinct
/// from the lazy [`ImportedMacroSurface`] payload, so a future
/// migration can rewrite producers structurally rather than
/// behaviourally.
#[derive(Debug, Clone)]
pub struct EagerResolvedMacro {
    pub macro_index: usize,
    pub macro_kind: AnalyzedMacroKind,
    pub type_name: String,
    pub import_source: String,
    pub surface_is_authoritative: bool,
    pub declaration: ResolvedTypeDeclaration,
    pub native_props: Vec<ResolvedNativeProp>,
    pub props: Vec<verter_semantic::analysis::AnalyzedPropField>,
    pub emits: Vec<verter_semantic::analysis::AnalyzedEmitField>,
    pub slots: Vec<verter_semantic::analysis::AnalyzedSlotField>,
    pub jsdoc: Option<ResolvedJsdocBlock>,
}

/// Enum envelope distinguishing eager and lazy macro-surface
/// resolutions.
///
/// The eager arm wraps the existing OXC-resolved-elements
/// payload (structurally mirrored by [`EagerResolvedMacro`]).
/// The lazy arm wraps the typed-IR bridge.
///
/// The eager payload is `Box`ed: [`EagerResolvedMacro`] carries
/// the full prop/emit/slot vectors (~330 bytes) while the lazy
/// bridge is identity-only (~48 bytes). Boxing the large arm
/// keeps the enum compact so callers that hold it on the stack
/// (or in collections) do not pay the eager footprint for every
/// `LazyImported` / `Empty` value.
#[derive(Debug, Clone)]
pub enum ResolvedMacroSurface {
    /// Eager OXC-resolved-elements surface — what the existing
    /// resolution rail produces today.
    Eager(Box<EagerResolvedMacro>),
    /// Lazy typed-IR-backed surface — what an imported-registry
    /// rail can produce when the consumer is ready to drive
    /// dispatch directly. The bridge holds declaration identity
    /// only; concrete projections happen on demand through
    /// [`ImportedMacroSurface::project_named_member`].
    LazyImported(ImportedMacroSurface),
    /// Degenerate "no macro surface" arm — used by callers that
    /// distinguish "macro absent" from "macro present but empty".
    Empty,
}

impl ResolvedMacroSurface {
    /// Wrap an existing eager [`crate::resolver_core::ResolvedMacroMeta`]
    /// surface as a [`ResolvedMacroSurface::Eager`].
    ///
    /// This is the seam between the OXC-resolved-elements rail (which
    /// produces `ResolvedMacroMeta`) and the shared macro-shape
    /// interpretation. Every macro-shape consumer wraps the matching
    /// `ResolvedMacroMeta` here before reading its members through the
    /// shared [`Self::prop_members`] / [`Self::emit_members`] /
    /// [`Self::slot_members`] accessors. The eager arm returns the
    /// stored fields verbatim, so the wrap-then-read round-trip is
    /// bit-identical to a direct `.props` / `.emits` / `.slots` read.
    #[must_use]
    pub(crate) fn from_eager_meta(meta: &crate::resolver_core::ResolvedMacroMeta) -> Self {
        ResolvedMacroSurface::Eager(Box::new(EagerResolvedMacro {
            macro_index: meta.macro_index,
            macro_kind: meta.macro_kind,
            type_name: meta.type_name.clone(),
            import_source: meta.import_source.clone(),
            surface_is_authoritative: meta.surface_is_authoritative,
            declaration: meta.declaration.clone(),
            native_props: meta.native_props.clone(),
            props: meta.props.clone(),
            emits: meta.emits.clone(),
            slots: meta.slots.clone(),
            jsdoc: meta.jsdoc.clone(),
        }))
    }

    /// The `defineProps` member set this surface contributes.
    ///
    /// **The single shared prop interpretation both arms feed.** Raw
    /// `keyof` candidate names ARE the prop candidates (codex
    /// per-macro-kind guidance); the shared shape producers
    /// (`synthesize_define_props_shape_from_known_surface_with_authority`)
    /// apply defaults / optionality / value-type projection on top.
    ///
    /// - [`Self::Eager`] → the stored `props` vector verbatim
    ///   (bit-identical to the pre-migration direct field read).
    /// - [`Self::LazyImported`] → resolve the imported declaration's
    ///   one-level surface and reconstruct one [`AnalyzedPropField`] per
    ///   named member, carrying the member's `optional` /
    ///   `declared_in_macro_type_arg` metadata and the lazily-projected
    ///   value `TypeExpr`. Call signatures are NOT props — they are
    ///   ignored here.
    /// - [`Self::Empty`] → empty.
    #[must_use]
    pub(crate) fn prop_members(&self, ctx: &dyn ResolverContext) -> Vec<AnalyzedPropField> {
        match self {
            ResolvedMacroSurface::Eager(eager) => eager.props.clone(),
            ResolvedMacroSurface::LazyImported(surface) => {
                // Props carry macro-T own-body provenance so the imported
                // declaration's own-body members surface with
                // `declared_in_macro_type_arg = true` (codex BINDING design)
                // (continued).
                let view = match surface.resolve_surface_view(
                    ctx,
                    crate::semantic_query::SurfaceProvenanceContext::MacroTypeArgOwnBody,
                ) {
                    QueryResult::Value(view) => view,
                    QueryResult::Error(_) | QueryResult::Recursive(_) => return Vec::new(),
                };
                let root_scope = TypeExprScope::new(surface.identity.canonical_id.as_ref());
                view.members
                    .iter()
                    .map(|member| {
                        let type_expr = ImportedMacroSurface::raise_member_value(ctx, member.value);
                        // Scope the paired `type_expr` to the member's ORIGIN
                        // file (see [`ImportedMacroSurface::member_expr_scope`]):
                        // for an inherited member the type-expr was written in
                        // the heritage base's file, so a nested `Ref` must
                        // resolve THERE, not in the root declaration's file.
                        let type_expr_scope = type_expr.is_some().then(|| {
                            ImportedMacroSurface::member_expr_scope(ctx, member.value, &root_scope)
                        });
                        // Display sidecar (cache-owned): the published
                        // `type_annotation` display string is rendered from
                        // the cache-owned typed `type_expr` — NOT re-parsed
                        // from source text (Typed-IR-Only rule: display text
                        // is a passthrough projection of the typed form, the
                        // inverse direction is the banned one). The JSDoc
                        // sidecar reattaches by member origin (see
                        // [`Self::member_display_jsdoc`]): for an inherited
                        // member the origin is the heritage base declaration's
                        // file (the member value node's scope), not the root
                        // declaration's file.
                        let type_annotation = type_expr.as_ref().and_then(
                            crate::resolver_core::surface_projector::render_type_expr_display,
                        );
                        let (description, tags) = ImportedMacroSurface::member_display_jsdoc(
                            ctx,
                            member.value,
                            member.name.as_ref(),
                        );
                        AnalyzedPropField {
                            name: member.name.as_ref().to_string(),
                            is_optional: member.optional,
                            span: verter_span::Span::default(),
                            type_annotation,
                            type_expr,
                            type_expr_scope,
                            description,
                            tags,
                            resolution_source:
                                verter_semantic::analysis::types::TypeResolutionSource::default(),
                            resolution_error: None,
                            declared_in_macro_type_arg: member.declared_in_macro_type_arg,
                        }
                    })
                    .collect()
            }
            ResolvedMacroSurface::Empty => Vec::new(),
        }
    }

    /// The `defineEmits` member set this surface contributes.
    ///
    /// **The single shared emit interpretation both arms feed.**
    /// Property-style keys come from raw member names, BUT
    /// **call-signature emits MUST extract event names from the call
    /// signatures, NOT from `keyof`** (codex BINDING guidance — a
    /// `{ (e: 'change', v: number): void }` surface has its event name
    /// in the first call-signature parameter, never in the member-name
    /// set). The shared shape producer
    /// (`synthesize_define_emits_shape_from_known_surface`) consumes the
    /// reconstructed field set unchanged.
    ///
    /// - [`Self::Eager`] → the stored `emits` vector verbatim (the eager
    ///   OXC rail already extracted call-signature event names into
    ///   `AnalyzedEmitField` records at parse time).
    /// - [`Self::LazyImported`] → reconstruct one [`AnalyzedEmitField`]
    ///   per property-style member name PLUS one per call-signature
    ///   event name (the call signature's first parameter literal — a
    ///   `String` literal or a `Union` of `String` literals — yields the
    ///   event name(s); the remaining parameters form the payload
    ///   tuple). De-duplicated by event name, first-writer-wins,
    ///   matching the eager projector's `retain`.
    /// - [`Self::Empty`] → empty.
    #[must_use]
    pub(crate) fn emit_members(&self, ctx: &dyn ResolverContext) -> Vec<AnalyzedEmitField> {
        match self {
            ResolvedMacroSurface::Eager(eager) => eager.emits.clone(),
            ResolvedMacroSurface::LazyImported(surface) => {
                // Emits are structural — `declared_in_macro_type_arg` is
                // a props-axis concern, always `false` for emit members.
                let view = match surface.resolve_surface_view(
                    ctx,
                    crate::semantic_query::SurfaceProvenanceContext::Structural,
                ) {
                    QueryResult::Value(view) => view,
                    QueryResult::Error(_) | QueryResult::Recursive(_) => return Vec::new(),
                };
                let root_scope = TypeExprScope::new(surface.identity.canonical_id.as_ref());
                let mut emits: Vec<AnalyzedEmitField> = Vec::new();

                // Call-signature emits: the event name is the FIRST
                // parameter's string literal (or union of string
                // literals); the payload is the call-signature function
                // with the event-name parameter STRIPPED — i.e.
                // `(e: 'change', v: number) => void` yields event
                // `change` with payload `(v: number) => void`.
                //
                // This mirrors the eager
                // `surface_projector::project_macro_surfaces` rail: OXC
                // resolves `elements.emits[i].type_expr` to the
                // event-name-stripped function, which the projector
                // copies verbatim into `payload_expr`. The lazy arm must
                // NOT read the event name from `keyof` (which would
                // surface numeric tuple indices, never the event name)
                // and must drop the leading event-name parameter so the
                // published payload TypeExpr matches the eager rail.
                for sig_node in &view.call_signatures {
                    let Some(TypeExpr::Function(func)) =
                        ImportedMacroSurface::raise_member_value(ctx, *sig_node)
                    else {
                        continue;
                    };
                    let Some(first) = func.parameters.first() else {
                        continue;
                    };
                    // Payload function: same return type + type params,
                    // parameters with the leading event-name parameter
                    // dropped.
                    // Dropping the leading event-name parameter preserves the
                    // function's OXC signature spans and the surviving
                    // parameters' spans (carried via the clone).
                    let payload_fn =
                        TypeExpr::Function(Arc::new(verter_type_expr::FunctionExpr::with_spans(
                            func.parameters.iter().skip(1).cloned().collect(),
                            func.return_type.clone(),
                            func.type_parameters.clone(),
                            func.spans,
                        )));
                    // Scope the payload to the call signature's ORIGIN file:
                    // an inherited call-signature emit carries its payload
                    // `Ref`s from the heritage base's file.
                    let payload_scope =
                        ImportedMacroSurface::member_expr_scope(ctx, *sig_node, &root_scope);
                    let mut push_event = |name: String| {
                        emits.push(AnalyzedEmitField {
                            name,
                            span: verter_span::Span::default(),
                            payload_type: None,
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

                // Property-style emits: each named member is an event,
                // its value type is the payload. This mirrors the eager
                // `surface_projector` precedence exactly — the
                // property-style fallback fires ONLY when no
                // call-signature emits were found (a mixed interface's
                // named members do NOT add events alongside its call
                // signatures).
                if emits.is_empty() {
                    for member in &view.members {
                        let payload_expr =
                            ImportedMacroSurface::raise_member_value(ctx, member.value);
                        // Scope the payload to the member's ORIGIN file (an
                        // inherited property-style emit's payload `Ref`s live
                        // in the heritage base's file).
                        let payload_expr_scope = payload_expr.is_some().then(|| {
                            ImportedMacroSurface::member_expr_scope(ctx, member.value, &root_scope)
                        });
                        // Display sidecar: payload text rendered from the
                        // cache-owned typed `payload_expr`; JSDoc reattached
                        // by member origin (see `member_display_jsdoc`).
                        let payload_type = payload_expr.as_ref().and_then(
                            crate::resolver_core::surface_projector::render_type_expr_display,
                        );
                        let (description, tags) = ImportedMacroSurface::member_display_jsdoc(
                            ctx,
                            member.value,
                            member.name.as_ref(),
                        );
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

                // De-duplicate by event name, first-writer-wins (the
                // eager projector applies the same `retain`).
                let mut seen = std::collections::HashSet::new();
                emits.retain(|emit| seen.insert(emit.name.clone()));
                emits
            }
            ResolvedMacroSurface::Empty => Vec::new(),
        }
    }

    /// The `defineSlots` member set this surface contributes.
    ///
    /// **The single shared slot interpretation both arms feed.** Raw
    /// member names are slot candidates ONLY (codex BINDING guidance);
    /// the lazy arm MUST keep function-like slot members and extract the
    /// binding / return shape, FILTERING non-function members. The
    /// shared shape producer
    /// (`synthesize_define_slots_shape_from_known_surface`) and the
    /// slot-binding graph consume the reconstructed slot fields.
    ///
    /// - [`Self::Eager`] → the stored `slots` vector verbatim.
    /// - [`Self::LazyImported`] → reconstruct one [`AnalyzedSlotField`]
    ///   per **function-like** member (the value raises to a
    ///   `TypeExpr::Function`); non-function members are filtered. The
    ///   slot's `bindings` come from the function's first parameter
    ///   object's properties (`binding_expr` = each property's value
    ///   type); the `return_expr` comes from the function's return type.
    /// - [`Self::Empty`] → empty.
    #[must_use]
    pub(crate) fn slot_members(&self, ctx: &dyn ResolverContext) -> Vec<AnalyzedSlotField> {
        match self {
            ResolvedMacroSurface::Eager(eager) => eager.slots.clone(),
            ResolvedMacroSurface::LazyImported(surface) => {
                // Slots are structural — `declared_in_macro_type_arg` is
                // a props-axis concern, always `false` for slot members.
                let view = match surface.resolve_surface_view(
                    ctx,
                    crate::semantic_query::SurfaceProvenanceContext::Structural,
                ) {
                    QueryResult::Value(view) => view,
                    QueryResult::Error(_) | QueryResult::Recursive(_) => return Vec::new(),
                };
                let root_scope = TypeExprScope::new(surface.identity.canonical_id.as_ref());
                view.members
                    .iter()
                    .filter_map(|member| {
                        // Slot members are function-like
                        // (`(props) => VNode[]`). A non-function member
                        // is NOT a slot — filter it out. The value is
                        // raised once; a non-`Function` raise drops the
                        // member.
                        let value = ImportedMacroSurface::raise_member_value(ctx, member.value)?;
                        let func = match &value {
                            TypeExpr::Function(func) => func,
                            _ => return None,
                        };
                        // Scope binding + return `*_expr` to the slot member's
                        // ORIGIN file: an inherited slot's binding/return
                        // `Ref`s were written in the heritage base's file.
                        let scope =
                            ImportedMacroSurface::member_expr_scope(ctx, member.value, &root_scope);
                        let bindings = func
                            .parameters
                            .first()
                            .map(|param| binding_fields_from_param_ty(&param.ty, &scope))
                            .unwrap_or_default();
                        let return_expr = func.return_type.as_ref().map(|rt| (**rt).clone());
                        let return_expr_scope = return_expr.is_some().then(|| scope.clone());
                        // Display sidecar: return-type text rendered from the
                        // cache-owned typed return; JSDoc reattached by slot
                        // member origin (see `member_display_jsdoc`).
                        let return_type = return_expr.as_ref().and_then(
                            crate::resolver_core::surface_projector::render_type_expr_display,
                        );
                        let (description, tags) = ImportedMacroSurface::member_display_jsdoc(
                            ctx,
                            member.value,
                            member.name.as_ref(),
                        );
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
            ResolvedMacroSurface::Empty => Vec::new(),
        }
    }
}

/// Reconstruct a slot's binding fields from its function's first
/// parameter type. The parameter is the slot props object
/// (`(props: { item: string }) => …`); each object property becomes one
/// [`AnalyzedSlotFieldBinding`] carrying the property's value `TypeExpr`
/// as `binding_expr`. A non-object parameter (or a function with no
/// parameter) yields no bindings — matching the eager rail, which only
/// records bindings for the slot-props-object form.
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
                // Display sidecar: binding type text rendered from the
                // cache-owned typed value (`prop.ty`) — the display
                // passthrough projection of the typed form.
                type_annotation: crate::resolver_core::surface_projector::render_type_expr_display(
                    &prop.ty,
                ),
                binding_expr: Some(prop.ty.clone()),
                binding_expr_scope: Some(scope.clone()),
                span: verter_span::Span::default(),
            }),
            _ => None,
        })
        .collect()
}

/// Bump the
/// [`verter_audit::AuditEvent::ImportedMacroSurfaceProjection`]
/// counter exactly once per public bridge dispatch entry
/// (`resolve_root`, `project_named_member`, `enumerate_member_names`).
///
/// Inlined so the call is a single TLS-observer fetch + atomic add
/// on the warm path — no allocation, no closure construction.
#[inline]
fn bump_projection_counter() {
    if let Some(observer) = verter_audit::current_observer() {
        observer.record_event(verter_audit::AuditEvent::ImportedMacroSurfaceProjection);
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests covering the bridge's invariants in isolation
    //! from the resolver pipeline.
    //!
    //! Hermetic dispatch behaviour lives in
    //! `crates/verter_session/tests/imported_macro_surface_bridge.rs`.

    use super::*;

    fn fixture_identity() -> ImportedDeclarationIdentity {
        ImportedDeclarationIdentity::new(
            Arc::from("/w/src/types.ts"),
            Arc::from("Props"),
            [0u8; 16],
        )
    }

    #[test]
    fn identity_top_level_scope_has_no_local_scope() {
        let id = fixture_identity();
        let scope = id.top_level_scope();
        assert_eq!(scope.canonical_id.as_ref(), "/w/src/types.ts");
        assert!(
            scope.local_scope.is_none(),
            "imported declaration roots MUST live at the top-level \
             scope — a `Some(_)` here would route ResolveDecl into a \
             block-local lookup that the imported-registry rail never \
             populates",
        );
    }

    #[test]
    fn identity_hash_eq_is_structural() {
        // Two identities with identical fields must hash and
        // compare equal so they collapse on the ResolveDecl memo.
        let a = fixture_identity();
        let b = fixture_identity();
        assert_eq!(a, b);

        let mut h_a = std::collections::hash_map::DefaultHasher::new();
        let mut h_b = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&a, &mut h_a);
        std::hash::Hash::hash(&b, &mut h_b);
        assert_eq!(
            std::hash::Hasher::finish(&h_a),
            std::hash::Hasher::finish(&h_b),
        );

        // Differing type_name must distinguish.
        let c = ImportedDeclarationIdentity::new(
            Arc::clone(&a.canonical_id),
            Arc::from("Different"),
            a.whole_hash,
        );
        assert_ne!(a, c);
    }

    #[test]
    fn surface_clones_share_identity() {
        let id = fixture_identity();
        let surface = ImportedMacroSurface::new(id.clone());
        let cloned = surface.clone();
        assert_eq!(surface.identity(), cloned.identity());
        assert_eq!(surface.identity(), &id);
    }

    #[test]
    fn resolved_macro_surface_variants_exist() {
        // Construction-only check — if any variant signature
        // changes the test fails to compile.
        let _ = ResolvedMacroSurface::Empty;
        let _ = ResolvedMacroSurface::LazyImported(ImportedMacroSurface::new(fixture_identity()));
    }
}
