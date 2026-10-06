//! Request-scoped memos of import-route witness builds and analysis-canonical
//! normalizations, dropped with the request context.

use verter_session_query::facts::fact_cache::FactVersionRef;

/// What resolving one owner's specifier set observed: whether a resolution
/// was refused, and every observation the admitted ones recorded, in order.
#[derive(Debug)]
pub struct ImportRouteObservation {
    pub refused: bool,
    pub observed: Vec<FactVersionRef>,
}

/// The identity of one witness build within a request: the host, the
/// owner, the specifier lanes resolved, and the generations a load or an
/// edit advances, so a build after either resolves again.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImportRouteObservationKey {
    pub host: usize,
    pub canonical: std::sync::Arc<str>,
    pub specifiers: Vec<(
        String,
        Option<verter_session_query::resolution::ResolveRequestKind>,
    )>,
    pub load_generation: u64,
    pub store_view_epoch: u64,
}

/// A request's witness builds, so every consumer that roots on an owner's
/// import-route witness within the request shares one resolution of the
/// owner's specifiers instead of resolving all of them again (the witness
/// is rooting evidence: a build that a later change in the request makes
/// stale fails validation, never answers wrongly). Owned by the
/// [`crate::request_context::RequestContext`] and dropped with it; bounded
/// by the owners the request roots on.
#[derive(Debug, Default)]
pub struct ImportRouteObservationMemo(
    pub  parking_lot::Mutex<
        rustc_hash::FxHashMap<ImportRouteObservationKey, std::sync::Arc<ImportRouteObservation>>,
    >,
);

/// One analysis-canonical normalization a request made: the canonical it
/// normalized to (`None`: to itself), whether a resolution it drove was
/// refused, and every observation those resolutions recorded.
#[derive(Debug)]
pub struct NormalizedCanonical {
    pub normalized: Option<std::sync::Arc<str>>,
    pub refused: bool,
    pub observed: Vec<FactVersionRef>,
}

/// The identity of one normalization within a request: the host, the
/// canonical, and the generations a load or an edit advances, so a
/// normalization after either probes again.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NormalizedCanonicalKey {
    pub host: usize,
    pub canonical: std::sync::Arc<str>,
    pub load_generation: u64,
    pub store_view_epoch: u64,
}

/// A request's analysis-canonical normalizations, so every consumer that
/// normalizes the same canonical in the request shares one run of its
/// declaration-companion probes (a normalization is rooting evidence
/// exactly like a witness build: its observations are replayed into the
/// witness scopes open around each consumer, and a refusal is re-noted).
/// Owned by the [`crate::request_context::RequestContext`] and dropped
/// with it; bounded by the canonicals the request normalizes.
#[derive(Debug, Default)]
pub struct NormalizedCanonicalMemo(
    pub  parking_lot::Mutex<
        rustc_hash::FxHashMap<NormalizedCanonicalKey, std::sync::Arc<NormalizedCanonical>>,
    >,
);
