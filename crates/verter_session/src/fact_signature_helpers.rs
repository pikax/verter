//! Shared helpers for the R3/R26/R28 fact-based validation substrate
//! used by the inner component-meta caches.
//!
//! Every cache entry that previously carried
//! [`crate::semantic_query::DepSignature`] migrates to
//! `Arc<[FactVersionRef]>` (the cold-compute observation set the
//! producer recorded). Warm-hit reads call
//! [`validate_fact_signature`] which walks every fact through the
//! current [`crate::resolver_core::StoreView`] snapshot; a single
//! mismatch returns `false` and the warm hit misses, falling through
//! to cold recompute.
//!
//! Bubble-up is the dual rule: when a cache cold-compute or warm-hit
//! returns a value to its caller, the entry's `fact_dep_signature`
//! merges into any active outer tracer via
//! [`crate::resolver_core::FactReadSetCell::observe_borrowed_signature`].
//! That keeps the outer compute's observation set complete even when
//! inner reads come from cache.
//!
//! ## Path-precise observation (R28)
//!
//! For caches keyed on `(canonical, name)` — the Family A pattern —
//! the cold path reads the `Member` body fingerprint and
//! `MemberPresence` header fact for `(canonical, name)` in the
//! `Type` symbol space. Adding a sibling member to `canonical` does
//! NOT shift `Member(canonical, name)` or `MemberPresence(canonical,
//! name)`, so unrelated edits do not invalidate the entry.
//!
//! For caches whose key shape does NOT include a member name (e.g.
//! `AppConfigNoOverrideProofKey`), the cold path observes the
//! `SyntacticExportSet` of the contributing canonical instead.
//! Whole-file `FileWholeHash` observations are reserved for callers
//! that genuinely consume the full file body (e.g. `<src=>` external
//! blocks); cross-file member-precise consumers route through this
//! module's `member_*` helpers.

use std::sync::Arc;

use verter_semantic::facts::registry::{FactKey, FactLane, InternedName, SymbolSpace};

use crate::resolver_core::{FactVersionRef, ParseFactRef, ResolverContext, StoreView};
use crate::types::Hash16;

/// Walk every `FactVersionRef` in `signature` against the current
/// resolver-store view; return `false` on the first mismatch.
///
/// `O(signature.len())`; zero allocation on the empty path. Empty
/// signatures trivially validate (callers that never observed a fact
/// have no R3 oracle to consult — typical for cache entries produced
/// outside an installed tracer scope; the cache stays correct under
/// the legacy whole-hash regime).
#[inline]
pub(crate) fn validate_fact_signature(
    ctx: &dyn ResolverContext,
    signature: &[FactVersionRef],
) -> bool {
    if signature.is_empty() {
        return true;
    }
    let view = ctx.resolver_store_view();
    signature.iter().all(|fact| view.validates(fact))
}

/// Bubble `signature` into the active fact tracer if one is
/// installed. Called by both cold-compute and warm-hit paths so an
/// outer compute's accumulated observation set sees every transitive
/// fact the inner cache hit / produced.
#[inline]
pub(crate) fn bubble_fact_signature(ctx: &dyn ResolverContext, signature: &[FactVersionRef]) {
    if signature.is_empty() {
        return;
    }
    if let Some(cell) = ctx.current_fact_tracer() {
        cell.observe_borrowed_signature(signature);
    }
}

/// Sentinel hash returned when the producer requests a fact that the
/// FileFacts registry hasn't materialised yet (e.g. cold-compute
/// races a parse that hasn't published yet). Validator reads against
/// the registry's actual hash; a sentinel records "MUST be absent"
/// semantics so a later population (or its absence) is still
/// discriminating.
#[inline]
fn zero_hash() -> Hash16 {
    [0u8; 16]
}

/// Recover the producer's hash for a parse-domain fact key on
/// `canonical_id`. Returns `None` only when the file has no
/// `FileArtifacts` entry under any `(content_hash, parse_env_hash)`
/// in the cache (truly absent file — the validator will treat the
/// missing fact as a miss).
fn lookup_parse_fact_hash(
    ctx: &dyn ResolverContext,
    canonical_id: &str,
    key: &FactKey,
    lane: FactLane,
) -> Option<Hash16> {
    let artifacts = ctx
        .project_type_store()
        .indexed()
        .get_artifacts_any(canonical_id)?;
    let fact = artifacts.facts.lookup(key)?;
    Some(match lane {
        FactLane::Semantic => fact.semantic_hash,
        FactLane::Display => fact.display_hash,
    })
}

/// Emit a `ParseFactRef` carrying the producer's current hash (or
/// the zero sentinel when the fact is absent from the registry).
fn parse_fact_ref(
    ctx: &dyn ResolverContext,
    canonical_id: &str,
    key: FactKey,
    lane: FactLane,
) -> FactVersionRef {
    let expected_hash =
        lookup_parse_fact_hash(ctx, canonical_id, &key, lane).unwrap_or_else(zero_hash);
    FactVersionRef::Parse(ParseFactRef {
        canonical_id: canonical_id.to_string(),
        key,
        lane,
        expected_hash,
    })
}

/// Build a path-precise signature for a cache whose lookup is
/// `(canonical, type_name)` in the `Type` symbol space — the
/// Family A producer pattern.
///
/// The cold-compute reads the type body at `(canonical, type_name)`,
/// so the signature observes the body fingerprint via the
/// `Export(name=type_name)` or `LocalDecl(name=type_name)` fact —
/// whichever the file emits for the named symbol. Both keys are
/// observed; only the one that actually exists carries a non-zero
/// hash. Adding sibling members or unrelated exports to the file
/// keeps these facts stable (R10 / R28); editing the named type's
/// body shifts the body hash; renaming or removing the type drops
/// both facts (validator treats absence as a sentinel-hash mismatch).
pub(crate) fn fact_signature_for_canonical_member(
    ctx: &dyn ResolverContext,
    canonical_id: &str,
    type_name: &str,
) -> Arc<[FactVersionRef]> {
    let name = InternedName::from(type_name);
    let space = SymbolSpace::Type;
    let export_key = FactKey::Export {
        name: name.clone(),
        space,
    };
    let local_decl_key = FactKey::LocalDecl { name, space };
    let entries: Vec<FactVersionRef> = vec![
        parse_fact_ref(ctx, canonical_id, export_key, FactLane::Semantic),
        parse_fact_ref(ctx, canonical_id, local_decl_key, FactLane::Semantic),
    ];
    Arc::from(entries)
}

/// Build a whole-canonical signature for caches whose cold-compute
/// reads the file's surface fingerprint (e.g. a binding-walker that
/// enumerates every export). Observes `SyntacticExportSet` — adding
/// or removing exports shifts the fact; cosmetic edits inside a
/// member body do NOT (they are observed by `Member(...)` facts on
/// each consumer).
pub(crate) fn fact_signature_for_canonical_surface(
    ctx: &dyn ResolverContext,
    canonical_id: &str,
) -> Arc<[FactVersionRef]> {
    let entries = vec![parse_fact_ref(
        ctx,
        canonical_id,
        FactKey::SyntacticExportSet,
        FactLane::Semantic,
    )];
    Arc::from(entries)
}

/// Empty signature constructor for cache entries published outside
/// any observable cold-compute pass (e.g. test fixtures, synthetic
/// publish paths). Validator trivially accepts; readers fall back to
/// the existing whole-hash regime in the legacy producer.
#[inline]
pub(crate) fn empty_fact_signature() -> Arc<[FactVersionRef]> {
    Arc::from(Vec::<FactVersionRef>::new())
}
