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
//! # Current behaviour
//!
//! The bridge is pure typed-IR infrastructure. The existing
//! eager resolution rail still produces
//! [`crate::resolver_core::ResolvedImportedMacroSurface`] for
//! every imported macro target — the bridge is added in
//! parallel as an opt-in surface for callers ready to drive
//! typed-IR dispatch directly. The
//! [`AuditEvent::ImportedMacroSurfaceProjection`] counter fires
//! once per public bridge accessor entry, so observability of
//! the bridge's fire rate is available immediately even before
//! a consumer adopts it.

use std::sync::Arc;

use verter_semantic::analysis::types::AnalyzedMacroKind;

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
