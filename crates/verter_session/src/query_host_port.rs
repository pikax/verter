//! Session-side implementation of the query-owned host port.
//!
//! [`verter_session_query::QueryHostPort`] is the inversion-of-control seam
//! between the query layer and the session host: the query layer OWNS the
//! trait and the neutral DTOs; this module implements the demand against
//! the session's real machinery. The adapter binds a
//! [`RequestBoundResolverContext`] — the sealed marker subtrait
//! implemented ONLY for the two request-bound contexts, so the retired
//! bare-host rail is structurally unbindable — and serves the canonical
//! post-parse artifact through the context's overlay-aware
//! `ensure_indexed_ready_serve` tier, so every serve is view-correct for
//! the requesting caller: a base context reads base artifacts, a session
//! context reads overlay-content-rooted artifacts under the
//! overlay-scoped key — and the publication status stamped onto the
//! port's admission signal reflects that same view. (One tracked
//! exception, §8.2: an explicit-overlay canonical whose materializer
//! declines still falls through to the base shared tier's
//! `store_published: true` — a pre-existing shared-tier fail-open this
//! port does NOT close, deferred to the P5 caller wiring plus the
//! real-adapter overlay regression.) The
//! authored-body-lowering demand
//! delegates to the decl-body memo's locator deref
//! ([`crate::decl_body_memo::DeclBodyMemo::deref_locator_body`]), whose
//! demanded lowering runs LEASE-ONLY through
//! [`crate::decl_lowering::DeclLoweringService::run_leased`] against the
//! scheduler-retained parse snapshot. The port adds NO second lowering path
//! and NO resolution of its own — it routes, delegates, maps the typed
//! product onto the neutral wire vocabulary, and carries the serve's
//! publication status across the boundary as the neutral cache-admission
//! signal on every outcome arm, success and failure alike.

use verter_session_query::{
    AuthoredBodyLowering, AuthoredBodyShape, QueryHostAdmission, QueryHostError, QueryHostPort,
    QueryHostServe,
};
use verter_type_expr::locators::AuthoredBodyLocator;

use crate::decl_body_memo::locator_deref::DerefedAuthoredBody;
use crate::decl_body_memo::{DerefedBodyShape, LocatorBodyDerefError};
use crate::resolver_core::RequestBoundResolverContext;

/// Host-backed adapter implementing the query layer's host port.
///
/// Binds a [`RequestBoundResolverContext`] — the sealed marker subtrait
/// implemented ONLY for a `HostResolverContext` (base query) or a
/// `SessionResolverContext` (overlay/session query), so the bare-host
/// rail cannot be bound at the type level; construct via
/// `SessionQueryHostPort::new`. Routing (anchor canonical → canonical
/// post-parse artifact → decl-body memo) goes through the context's
/// `ensure_indexed_ready_serve` — the single, overlay-aware
/// materialization bridge: a session view serves overlay-content-rooted
/// artifacts under the overlay-scoped key, a base context serves base
/// artifacts, so the serve AND its publication status are always
/// correct for the requesting view. That by-value
/// publication status flows onto EVERY port answer, success and failure
/// alike, as the neutral [`QueryHostAdmission`] — a FENCED (ReturnOnly)
/// serve answers the requesting caller's read but must never seed a
/// shared cache, and a genuine miss derefed against that fenced surface
/// is equally return-only — so the completion fence survives the query
/// boundary instead of stopping at the request-sticky / traced-scope
/// suppression rails inside the bridge.
pub struct SessionQueryHostPort<'ctx> {
    ctx: &'ctx dyn RequestBoundResolverContext,
}

impl<'ctx> SessionQueryHostPort<'ctx> {
    /// Bind the query host port to a [`RequestBoundResolverContext`] so
    /// every artifact serve is overlay-correct for the requesting view
    /// (a `HostResolverContext` for a base query, a
    /// `SessionResolverContext` for an overlay/session query). The
    /// completion-fence admission then reflects the correct view.
    ///
    /// The request-bound binding is STRUCTURAL: the parameter type admits
    /// only the sealed request-bound marker's implementers, so the retired
    /// bare-host rail (`&VerterHost`) cannot be passed — it implements
    /// [`ResolverContext`](crate::resolver_core::ResolverContext) but not
    /// the marker. The `debug_assert` below is
    /// therefore redundant defense-in-depth (it can only ever hold),
    /// retained so a hypothetical future marker misuse trips loudly in dev
    /// builds.
    pub(crate) fn new(ctx: &'ctx dyn RequestBoundResolverContext) -> Self {
        debug_assert!(
            ctx.is_request_bound(),
            "QueryHostPort binds a request-view-bound ResolverContext"
        );
        Self { ctx }
    }
}

// ── Request-bound seal: structural discrimination ────────────────────
//
// `SessionQueryHostPort::new` binds `&dyn RequestBoundResolverContext`, so
// the retired bare-host rail is UNCONSTRUCTIBLE at the type level. These
// compile-time proofs discriminate the seal in both directions and are
// checked by the ordinary `cargo check -p verter_session` build — the
// enforcement is the type system itself, not a runtime test case. (A
// trybuild fixture cannot discriminate this: `new` is `pub(crate)`, so an
// external fixture fails on the pre-existing visibility wall regardless
// of the marker, which would not distinguish a sealed from an unsealed
// binding.)

// NEGATIVE — the bare host implements `ResolverContext` but NOT the
// request-bound marker, so `&VerterHost` cannot coerce to the port's
// binding. A launder (`impl RequestBoundResolverContext for VerterHost`)
// would make this assertion fail to compile, widening the baseline error
// count.
static_assertions::assert_not_impl_all!(crate::VerterHost: RequestBoundResolverContext);

const _: () = {
    // POSITIVE — both genuinely request-bound contexts coerce to the
    // port's `&dyn RequestBoundResolverContext` binding and construct the
    // port. If either context's marker impl regressed, these would fail to
    // compile.
    fn host_ctx_constructs_port<'a>(
        ctx: &'a crate::resolver_core::HostResolverContext<'a>,
    ) -> SessionQueryHostPort<'a> {
        SessionQueryHostPort::new(ctx)
    }
    fn session_ctx_constructs_port<'a>(
        ctx: &'a crate::resolver_core::SessionResolverContext<'a>,
    ) -> SessionQueryHostPort<'a> {
        SessionQueryHostPort::new(ctx)
    }
    // Reference both so the compile-time proofs are not dead code.
    let _ = host_ctx_constructs_port;
    let _ = session_ctx_constructs_port;
};

impl QueryHostPort for SessionQueryHostPort<'_> {
    fn lower_authored_body(&self, locator: &AuthoredBodyLocator) -> QueryHostServe {
        // A locator derefs through the memo of its OWN producing canonical;
        // the anchor names that canonical for every locator kind.
        let anchor = match locator {
            AuthoredBodyLocator::DeclBody(slot) => &slot.anchor,
            AuthoredBodyLocator::AugmentationBody(aug) => &aug.anchor,
            AuthoredBodyLocator::JsdocTypedefBody(typedef) => &typedef.anchor,
            AuthoredBodyLocator::MacroPayload(payload) => &payload.anchor,
        };
        // The single materialization bridge for the canonical post-parse
        // artifact. `None` = the producing canonical is unknown to the live
        // view: a transient no-warm non-result on BOTH axes — no serve was
        // produced, so there is no publication status to map and nothing
        // derived from this answer may be admitted warm.
        let Some(serve) = self
            .ctx
            .ensure_indexed_ready_serve(anchor.canonical_id.as_ref())
        else {
            return QueryHostServe {
                admission: QueryHostAdmission::ReturnOnly,
                outcome: Err(QueryHostError::UnknownFile),
            };
        };
        // The serve's by-value publication status IS the port's admission
        // signal: `store_published == false` marks a FENCED flight that
        // published nothing — its answers (the lowering AND any genuine
        // miss derefed against the fenced surface) serve this caller's
        // read only and must never be admitted warm into a shared cache.
        // The admission rides the wrapper for BOTH outcome arms; the error
        // CLASS stays orthogonal and is never reclassified to smuggle the
        // fence through.
        let admission = QueryHostAdmission::from_store_published(serve.store_published);
        let outcome = serve
            .indexed
            .shallow_state
            .decl_bodies()
            .deref_locator_body(locator)
            .map(neutral_lowering)
            .map_err(neutral_error);
        QueryHostServe { admission, outcome }
    }
}

/// Maps the memo's owned deref product onto the port's neutral DTO — a 1:1
/// structural map (both sides carry the same lower-crate typed IR; the
/// merged-contributor carrier stays distinct, never an intersection).
fn neutral_lowering(derefed: DerefedAuthoredBody) -> AuthoredBodyLowering {
    AuthoredBodyLowering {
        shape: match derefed.shape {
            DerefedBodyShape::Single(body) => AuthoredBodyShape::Single(body),
            DerefedBodyShape::Merged(contributors) => AuthoredBodyShape::Merged(contributors),
        },
        type_parameters: derefed.type_parameters,
        visibility: derefed.visibility,
    }
}

/// Maps the session's typed deref failures onto the neutral port
/// vocabulary. EXHAUSTIVE by design — a new deref-error variant forces an
/// explicit cache-semantics classification here. The load-bearing classes
/// survive the mapping: genuine cacheable results (`UnknownSymbol`, the
/// authored-absence pair), the transient no-warm lease signal (never
/// collapsed into a cacheable miss), and the structural fail-closed
/// non-results.
fn neutral_error(error: LocatorBodyDerefError) -> QueryHostError {
    match error {
        LocatorBodyDerefError::UnknownSymbol => QueryHostError::UnknownSymbol,
        LocatorBodyDerefError::LeaseMiss => QueryHostError::LeaseMiss,
        LocatorBodyDerefError::ValueAnnotationAbsent
        | LocatorBodyDerefError::TypeParamBoundAbsent { .. } => QueryHostError::AuthoredBodyAbsent,
        LocatorBodyDerefError::CanonicalMismatch
        | LocatorBodyDerefError::OwnerMismatch
        | LocatorBodyDerefError::PathUnresolved
        | LocatorBodyDerefError::TypeParamOrdinalOutOfRange { .. }
        | LocatorBodyDerefError::TypeParamBoundStepMisplaced
        | LocatorBodyDerefError::NamespaceBodyUnrouted
        | LocatorBodyDerefError::AugmentationBodySpaceUnrouted
        | LocatorBodyDerefError::MacroTypeArgumentHasSoleHotMirrorProducer
        | LocatorBodyDerefError::MacroPayloadPositionUnrouted => QueryHostError::LocatorUnroutable,
    }
}
