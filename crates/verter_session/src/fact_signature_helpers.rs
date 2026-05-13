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
//! Two helper shapes serve the Family A path-precise observation
//! contract:
//!
//! - [`fact_signature_for_canonical_member`] — keyed on `(canonical,
//!   exporter, member, space)`. The cold path reads the `Member`
//!   body fingerprint and `MemberPresence` header fact for the
//!   member. Adding a sibling member to `exporter` does NOT shift
//!   either fact for the named member, so unrelated edits do not
//!   invalidate the entry.
//! - [`fact_signature_for_exported_type`] — keyed on `(canonical,
//!   type_name, space)`. The cold path observes the top-level type
//!   identity via `Export`, `LocalDecl`, and `MemberShape` facts.
//!   Editing a single member body does NOT invalidate (callers that
//!   walk member bodies must combine with `member` observations);
//!   adding/removing/renaming a member shifts `MemberShape` and
//!   invalidates whole-type readers.
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

/// Build a path-precise signature for a cache whose validity depends
/// on a single MEMBER of an exporter type — the R28 path-precise
/// pattern.
///
/// The cold-compute reads the body of `member` declared inside the
/// `exporter` type at `canonical_id`, so the signature observes BOTH
/// `MemberPresence(exporter, member, space)` (header fact — bumps on
/// add/remove/rename/kind-change) AND `Member(exporter, member,
/// space)` (body fingerprint — bumps on body edit). Adding sibling
/// members to `exporter` does NOT shift either fact for the named
/// member (R10 / R28); editing the named member's body shifts only
/// the `Member` body fact; removing the named member drops both
/// facts (validator treats absence as a sentinel-hash mismatch).
///
/// Use this helper for caches keyed on `(canonical, exporter,
/// member, space)` — e.g. `PreparedMemberDb`, slot-binding member
/// reads, fallthrough member projection.
pub(crate) fn fact_signature_for_canonical_member(
    ctx: &dyn ResolverContext,
    canonical_id: &str,
    exporter: &str,
    member: &str,
    space: SymbolSpace,
) -> Arc<[FactVersionRef]> {
    let exporter_name = InternedName::from(exporter);
    let member_name = InternedName::from(member);
    let presence_key = FactKey::MemberPresence {
        exporter: exporter_name.clone(),
        name: member_name.clone(),
        space,
    };
    let body_key = FactKey::Member {
        exporter: exporter_name,
        name: member_name,
        space,
    };
    let entries: Vec<FactVersionRef> = vec![
        parse_fact_ref(ctx, canonical_id, presence_key, FactLane::Semantic),
        parse_fact_ref(ctx, canonical_id, body_key, FactLane::Semantic),
    ];
    Arc::from(entries)
}

/// Build a signature for a cache whose validity depends on the
/// IDENTITY of a top-level type declared at `canonical_id` — the
/// Family A producer pattern for caches keyed on `(canonical,
/// type_name)`.
///
/// The cold-compute consumes the declaration of `type_name` (its
/// declaration shape and member list), so the signature observes:
/// - `Export(name, space)` — present iff the type is exported under
///   that name.
/// - `LocalDecl(name, space)` — present iff the type is declared
///   locally (non-exported).
/// - `MemberShape(exporter=name, space)` — the ordered member list
///   fingerprint; bumps when members are added/removed/renamed.
///
/// Editing one member's body changes `Member(name, m, space)` but
/// NOT this signature — that path-precise invalidation is the
/// caller's responsibility via [`fact_signature_for_canonical_member`].
/// This helper covers caches whose validity is "the top-level type
/// exists and has THIS member-shape", not "the body of a particular
/// member".
pub(crate) fn fact_signature_for_exported_type(
    ctx: &dyn ResolverContext,
    canonical_id: &str,
    type_name: &str,
    space: SymbolSpace,
) -> Arc<[FactVersionRef]> {
    let name = InternedName::from(type_name);
    let export_key = FactKey::Export {
        name: name.clone(),
        space,
    };
    let local_decl_key = FactKey::LocalDecl {
        name: name.clone(),
        space,
    };
    let member_shape_key = FactKey::MemberShape {
        exporter: name,
        space,
    };
    let entries: Vec<FactVersionRef> = vec![
        parse_fact_ref(ctx, canonical_id, export_key, FactLane::Semantic),
        parse_fact_ref(ctx, canonical_id, local_decl_key, FactLane::Semantic),
        parse_fact_ref(ctx, canonical_id, member_shape_key, FactLane::Semantic),
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
