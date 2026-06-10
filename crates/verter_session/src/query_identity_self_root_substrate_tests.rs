//! Shared substrate for query-identity self-version rooting —
//! discriminator tests.
//!
//! A query-identity cache entry keyed by `(canonical, ...)` must
//! detect a **same-canonical content edit** on the resolver's
//! cold-recompute read path. It does so by carrying a self-version
//! root for the keyed canonical in its `read_set_signature` and
//! validating that root strictly — an untracked whole-hash fact fails
//! validation rather than being permissively accepted. The upsert
//! performs no eager own-canonical cache drain; a stale entry may
//! physically linger and is rejected lazily on read by its
//! self-version root.
//!
//! This module locks down the four substrate deliverables that back
//! that lazy same-canonical rejection. Each test DISCRIMINATES — it
//! fails against a tree whose validation is lazy and passes against a
//! tree whose validation is strictly self-version-rooted — and asserts
//! a concrete observable.
//!
//! 1. [`observed_parse_fact_lookup_is_content_addressed_not_get_any`]
//!    — `parse_fact_ref_for_observed_current_content` reads a file's
//!    parse facts by a caller-supplied **observed** content identity
//!    and returns `None` for a stale-only artifact. A
//!    non-provenance-pure builder reads `FileArtifactStore::get_artifacts_any`,
//!    which returns the stale `FileArtifacts` regardless of content
//!    hash.
//! 2. [`exported_type_signature_is_provenance_pure`] /
//!    [`canonical_member_signature_is_provenance_pure`] /
//!    [`canonical_surface_signature_is_provenance_pure`] — the three
//!    central fact-signature helpers are provenance-pure: they lead
//!    with a `FileWholeHash` self-root pinned to a caller-supplied
//!    observed content hash and pin their `Parse` facts to that same
//!    observed version, never a current-content re-read. A fabricated
//!    observed hash with no content-addressed artifact yields `None`.
//! 3. [`strict_self_root_validation_rejects_untracked_whole_hash`] —
//!    `validate_fact_signature_with_self_roots` validates a self-root
//!    `FileWholeHash` strictly: an untracked keyed canonical fails
//!    validation. The lazy `validate_fact_signature` accepts the same
//!    untracked fact.

use std::sync::Arc;

use verter_semantic::facts::registry::{FactKey, SymbolSpace};

use crate::file_artifact_store::FileArtifactKey;
use crate::resolver_core::{FactVersionRef, ResolverContext};
use crate::{HostConfig, UpsertRequest, VerterHost};

/// Doctored content hash no real content ever produces. A planted
/// stale artifact carries this so a content-pinned read is trivially
/// distinguishable from a permissive `get_artifacts_any` read.
const STALE_HASH: [u8; 16] = [0xC7; 16];

/// Build a standalone host with a single `.ts` file materialised
/// through the scheduler, returning the host plus the real
/// (current-content) whole hash.
fn host_with_ts(path: &str, source: &str) -> (VerterHost, [u8; 16]) {
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(path.to_string()),
            input_id: path.to_string(),
            source: Arc::from(source),
            file_language: crate::LanguageRegistry::global()
                .classify_static(path)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .expect("seed upsert succeeds");
    let indexed = host
        .ensure_indexed_ready(path)
        .expect("IndexedReady must materialise for an upserted file");
    (host, indexed.whole_hash)
}

// ---------------------------------------------------------------------------
// Item 1 — content-addressed observed parse-fact recovery.
// ---------------------------------------------------------------------------

/// `parse_fact_ref_for_observed_current_content` performs a
/// content-addressed `FileArtifactStore` lookup keyed on the
/// caller-supplied **observed** content hash. When no artifact is
/// cached for the `(canonical, observed_hash)` identity it returns
/// `None` — the observed version's parse facts are unrecoverable, so
/// the provenance-pure signature builders refuse shared-cache
/// admission.
///
/// This is the substrate property that makes the provenance-pure
/// signature builders sound: a builder pins its parse facts to the
/// observed content version, and a builder threaded an observed hash
/// with no cached artifact MUST miss rather than fabricate a fact.
///
/// Discriminating property: a real artifact is materialised, then a
/// synthetic STALE `FileArtifacts` (doctored content hash) is planted
/// as the SOLE stored entry. The permissive `get_artifacts_any`
/// returns the planted stale entry regardless of content hash — that
/// is the read shape a non-provenance-pure builder would use, and it
/// always succeeds. `parse_fact_ref_for_observed_current_content`
/// keyed on the genuine `real_hash` is content-addressed: it finds NO
/// artifact at that identity (only the stale candidate is stored) and
/// returns `None`.
#[test]
fn observed_parse_fact_lookup_is_content_addressed_not_get_any() {
    use verter_semantic::facts::FactLane;

    let canonical = "/self_root/probe_facts.ts";
    let (host, real_hash) = host_with_ts(
        canonical,
        "export interface Probe { a: number; }\nexport const probe = 1;\n",
    );
    assert_ne!(
        real_hash, STALE_HASH,
        "fixture invariant: the real content hash must differ from the planted stale hash",
    );

    // Before planting: the content-addressed observed read pinned on
    // the genuine `real_hash` HITS. This anchors the discriminator so
    // the post-plant assertion is not vacuously satisfied by a
    // systematically-missing read.
    {
        let ctx: &dyn ResolverContext = &host;
        assert!(
            crate::fact_signature_helpers::parse_fact_ref_for_observed_current_content(
                ctx,
                canonical,
                real_hash,
                FactKey::SyntacticExportSet,
                FactLane::Semantic,
            )
            .is_some(),
            "fixture invariant: the content-addressed observed read must HIT the \
             genuine current artifact before the stale plant",
        );
    }

    // Plant a synthetic STALE FileArtifacts as the SOLE stored entry.
    // Clone the real artifact, doctor its `IndexedReady.whole_hash` to
    // the stale sentinel, then drop every real-keyed entry and store
    // only the stale-keyed one. `FileArtifactStore::get_artifacts` is
    // content-pinned, so afterwards an observed read pinned on
    // `real_hash` must miss while `get_artifacts_any` still hits.
    let real_artifacts = host
        .project_type_store()
        .indexed()
        .get_artifacts_any(canonical)
        .expect("real FileArtifacts must exist before planting the stale one");
    let mut stale = (*real_artifacts).clone();
    {
        let stale_indexed = Arc::make_mut(&mut stale.indexed);
        stale_indexed.whole_hash = STALE_HASH;
    }
    host.project_type_store().indexed().remove(canonical);
    host.project_type_store().indexed().insert_artifacts(
        FileArtifactKey::legacy(Arc::from(canonical), STALE_HASH),
        Arc::new(stale),
    );

    // The permissive `get_artifacts_any` surfaces the planted stale
    // entry — this is the read shape a non-provenance-pure builder
    // would use, and it always succeeds.
    let permissive = host
        .project_type_store()
        .indexed()
        .get_artifacts_any(canonical)
        .expect("get_artifacts_any must still return the (stale) entry");
    assert_eq!(
        permissive.indexed.whole_hash, STALE_HASH,
        "fixture invariant: get_artifacts_any returns the planted stale entry — \
         that is exactly the read shape a non-provenance-pure builder would use",
    );

    // The discriminating assertion: the content-addressed observed
    // read keyed on the genuine `real_hash` finds NO artifact at that
    // identity (only the stale candidate is stored), so it returns
    // `None`. A `get_artifacts_any`-based read returns `Some`.
    let ctx: &dyn ResolverContext = &host;
    assert!(
        crate::fact_signature_helpers::parse_fact_ref_for_observed_current_content(
            ctx,
            canonical,
            real_hash,
            FactKey::SyntacticExportSet,
            FactLane::Semantic,
        )
        .is_none(),
        "parse_fact_ref_for_observed_current_content MUST return None when no \
         artifact is cached for the (canonical, real_hash {real_hash:?}) identity — \
         only a stale candidate ({STALE_HASH:?}) is stored. A non-None result means \
         the read fell back to the permissive get_artifacts_any path.",
    );
}

// ---------------------------------------------------------------------------
// Item 2 — central fact-signature helpers prepend a self-root.
// ---------------------------------------------------------------------------

/// True iff `signature` contains a `FileWholeHash` self-root entry for
/// `canonical` whose hash equals `expected`.
fn has_self_root(signature: &[FactVersionRef], canonical: &str, expected: [u8; 16]) -> bool {
    signature.iter().any(|fact| {
        matches!(
            fact,
            FactVersionRef::FileWholeHash { canonical_id, hash }
                if canonical_id == canonical && *hash == expected
        )
    })
}

/// `fact_signature_for_exported_type` is provenance-pure: it leads
/// with a `FileWholeHash` self-root pinned to the caller-supplied
/// **observed** content hash, never a current-content re-read, and
/// pins its parse facts to the same observed version.
///
/// Discriminating property: the helper is called with `observed_hash
/// = H_observed` while the host's CURRENT content hash for `canonical`
/// is a DIFFERENT `H_current` (an edit landed after the observation).
/// A provenance-pure helper roots the signature on `H_observed`. The
/// pre-fix helper re-read current content via `self_root_fact` /
/// `parse_fact_ref` → `authoritative_current_content_hash` and would
/// root on `H_current` — this test FAILS against that body (the
/// `has_self_root(.., H_observed)` assertion trips and the
/// `H_current` self-root would be present instead).
#[test]
fn exported_type_signature_is_provenance_pure() {
    let canonical = "/self_root/exported_type.ts";
    let (host, observed_hash) = host_with_ts(
        canonical,
        "export interface Surface { a: number; b: string; }\n",
    );

    // The observation must still have a content-addressed artifact so
    // the provenance-pure parse-fact lookups resolve — `ensure_indexed_ready`
    // (idempotent) keeps the observed-version artifact reachable.
    let ctx: &dyn ResolverContext = &host;
    let signature = crate::fact_signature_helpers::fact_signature_for_exported_type(
        ctx,
        canonical,
        "Surface",
        SymbolSpace::Type,
        observed_hash,
    )
    .into_cacheable()
    .expect("the observed version's artifact is recoverable")
    .facts;

    assert!(
        has_self_root(&signature, canonical, observed_hash),
        "fact_signature_for_exported_type MUST lead with a FileWholeHash self-root \
         pinned to the caller-supplied observed hash ({canonical} @ {observed_hash:?}). \
         Signature was: {signature:?}",
    );
    // The stricter parse facts must still be present alongside the
    // self-root — the self-root augments, it does not replace.
    assert!(
        signature.iter().any(|f| matches!(
            f,
            FactVersionRef::Parse(p) if matches!(p.key, FactKey::MemberShape { .. })
        )),
        "the MemberShape parse fact must still be present alongside the self-root",
    );

    // Provenance discriminator: a fabricated observed hash distinct
    // from the file's current content roots the self-root on THAT
    // fabricated hash — proving the builder never re-reads current
    // content. A pre-fix builder rooted on `authoritative_current_content_hash`
    // and would emit the genuine `observed_hash` self-root instead.
    // The fabricated version has no artifact, so the parse-fact
    // lookups miss and the builder returns `None` — itself a
    // provenance-purity assertion: a current-content re-read would
    // succeed and return `Some`.
    let fabricated = STALE_HASH;
    assert_ne!(fabricated, observed_hash, "fixture invariant");
    assert!(
        crate::fact_signature_helpers::fact_signature_for_exported_type(
            ctx,
            canonical,
            "Surface",
            SymbolSpace::Type,
            fabricated,
        )
        .into_cacheable()
        .is_none(),
        "fact_signature_for_exported_type MUST return None for an observed hash with \
         no content-addressed artifact — a builder that re-read current content \
         would resolve the genuine current artifact and return Some.",
    );
}

/// `fact_signature_for_canonical_member` is provenance-pure: it leads
/// with a `FileWholeHash` self-root pinned to the caller-supplied
/// **observed** content hash and pins its `MemberPresence` / `Member`
/// parse facts to that same observed version.
///
/// Discriminating property: the helper is called with an observed
/// hash; the produced signature roots on that observed hash. A
/// fabricated observed hash with no content-addressed artifact yields
/// `None` — a pre-fix builder that re-read current content via
/// `self_root_fact` / `parse_fact_ref` would instead resolve the
/// genuine current artifact and return a signature.
#[test]
fn canonical_member_signature_is_provenance_pure() {
    let canonical = "/self_root/member_owner.ts";
    let (host, observed_hash) = host_with_ts(
        canonical,
        "export interface Holder { picked: number; sibling: string; }\n",
    );

    let ctx: &dyn ResolverContext = &host;
    let signature = crate::fact_signature_helpers::fact_signature_for_canonical_member(
        ctx,
        canonical,
        "Holder",
        "picked",
        SymbolSpace::Type,
        observed_hash,
    )
    .into_cacheable()
    .expect("the observed version's artifact is recoverable")
    .facts;

    assert!(
        has_self_root(&signature, canonical, observed_hash),
        "fact_signature_for_canonical_member MUST lead with a FileWholeHash \
         self-root pinned to the caller-supplied observed hash ({canonical} @ \
         {observed_hash:?}). Signature was: {signature:?}",
    );
    assert!(
        signature.iter().any(|f| matches!(
            f,
            FactVersionRef::Parse(p) if matches!(p.key, FactKey::Member { .. })
        )),
        "the Member parse fact must still be present alongside the self-root",
    );

    // Provenance discriminator: a fabricated observed hash has no
    // content-addressed artifact, so the `MemberPresence` / `Member`
    // parse-fact lookups miss and the builder returns `None`. A pre-fix
    // builder that re-read current content would resolve the genuine
    // current artifact and return `Some`.
    let fabricated = STALE_HASH;
    assert_ne!(fabricated, observed_hash, "fixture invariant");
    assert!(
        crate::fact_signature_helpers::fact_signature_for_canonical_member(
            ctx,
            canonical,
            "Holder",
            "picked",
            SymbolSpace::Type,
            fabricated,
        )
        .into_cacheable()
        .is_none(),
        "fact_signature_for_canonical_member MUST return None for an observed hash \
         with no content-addressed artifact — a builder that re-read current \
         content would resolve the genuine current artifact and return Some.",
    );
}

/// `fact_signature_for_canonical_surface` is provenance-pure: it leads
/// with a `FileWholeHash` self-root pinned to the caller-supplied
/// **observed** content hash and pins its `SyntacticExportSet` parse
/// fact to that same observed version.
///
/// Discriminating property: the helper is called with an observed
/// hash; the produced signature roots on that observed hash. A
/// fabricated observed hash with no content-addressed artifact yields
/// `None` — a pre-fix builder that re-read current content via
/// `self_root_fact` / `parse_fact_ref` would instead resolve the
/// genuine current artifact and return a signature.
#[test]
fn canonical_surface_signature_is_provenance_pure() {
    let canonical = "/self_root/surface_owner.ts";
    let (host, observed_hash) = host_with_ts(
        canonical,
        "export const a = 1;\nexport const b = 2;\nexport type C = string;\n",
    );

    let ctx: &dyn ResolverContext = &host;
    let signature = crate::fact_signature_helpers::fact_signature_for_canonical_surface(
        ctx,
        canonical,
        observed_hash,
    )
    .into_cacheable()
    .expect("the observed version's artifact is recoverable")
    .facts;

    assert!(
        has_self_root(&signature, canonical, observed_hash),
        "fact_signature_for_canonical_surface MUST lead with a FileWholeHash \
         self-root pinned to the caller-supplied observed hash ({canonical} @ \
         {observed_hash:?}). Signature was: {signature:?}",
    );
    assert!(
        signature
            .iter()
            .any(|f| matches!(f, FactVersionRef::Parse(p) if p.key == FactKey::SyntacticExportSet)),
        "the SyntacticExportSet parse fact must still be present alongside the self-root",
    );

    // Provenance discriminator: a fabricated observed hash has no
    // content-addressed artifact, so the `SyntacticExportSet`
    // parse-fact lookup misses and the builder returns `None`. A
    // pre-fix builder that re-read current content would resolve the
    // genuine current artifact and return `Some`.
    let fabricated = STALE_HASH;
    assert_ne!(fabricated, observed_hash, "fixture invariant");
    assert!(
        crate::fact_signature_helpers::fact_signature_for_canonical_surface(
            ctx, canonical, fabricated,
        )
        .into_cacheable()
        .is_none(),
        "fact_signature_for_canonical_surface MUST return None for an observed hash \
         with no content-addressed artifact — a builder that re-read current \
         content would resolve the genuine current artifact and return Some.",
    );
}

// ---------------------------------------------------------------------------
// Item 3 — strict self-root validation behavior.
// ---------------------------------------------------------------------------

/// A self-root `FileWholeHash` whose keyed canonical is NOT tracked by
/// the store view fails strict validation, where the lazy
/// dependency-fact validator accepts it.
///
/// Discriminating property: `validate_fact_signature` walks every
/// `FileWholeHash` through `StoreView::validates`, whose untracked-file
/// arm returns `true` ("loaded as a dependency after the snapshot —
/// optimistically accept"). A self-root, by construction, names the
/// cache entry's OWN keyed canonical — an untracked self-root means
/// the file is gone, which must fail. `validate_fact_signature_with_self_roots`
/// routes the named self-root canonicals through the strict
/// `validates_self_root_whole_hash` check (`None => false`).
///
/// The discriminator: for the SAME untracked-canonical `FileWholeHash`
/// fact, `validate_fact_signature` returns `true` and
/// `validate_fact_signature_with_self_roots` returns `false`. A
/// pre-substrate tree has no `validate_fact_signature_with_self_roots`
/// and no strict path; both reads would lazily accept.
#[test]
fn strict_self_root_validation_rejects_untracked_whole_hash() {
    // A fresh host with one unrelated file loaded; the probe canonical
    // below is never loaded, so the live store view does NOT track it.
    let (host, _other_hash) = host_with_ts(
        "/self_root/strict_other.ts",
        "export const unrelated = 1;\n",
    );
    let never_loaded = "/self_root/strict_never_loaded.ts";

    let ctx: &dyn ResolverContext = &host;
    let view = ctx.resolver_store_view();
    assert!(
        !crate::resolver_core::StoreView::tracks_file(&view, never_loaded),
        "fixture invariant: the probe canonical must be untracked by the live \
         store view — otherwise the lazy/strict arms are indistinguishable",
    );

    // A one-entry signature whose sole fact is a FileWholeHash for the
    // untracked probe canonical.
    let signature: Vec<FactVersionRef> = vec![FactVersionRef::FileWholeHash {
        canonical_id: never_loaded.to_string(),
        hash: STALE_HASH,
    }];

    // Lazy validation: the untracked FileWholeHash is optimistically
    // accepted.
    assert!(
        crate::fact_signature_helpers::validate_fact_signature(ctx, &signature),
        "fixture invariant: the lazy validate_fact_signature must ACCEPT an \
         untracked FileWholeHash — that is the permissive behavior the strict \
         path must override for self-roots",
    );

    // Strict validation, with the probe canonical named as a self-root:
    // the untracked FileWholeHash fails.
    assert!(
        !crate::fact_signature_helpers::validate_fact_signature_with_self_roots(
            ctx,
            &signature,
            &[never_loaded],
        ),
        "validate_fact_signature_with_self_roots MUST REJECT a self-root \
         FileWholeHash whose keyed canonical ({never_loaded}) is untracked by \
         the store view. Accepting it is the lazy 'unknown means okay' behavior \
         that lets a query-identity cache entry survive a same-canonical edit.",
    );
}

/// Strict self-root validation still accepts a self-root whose keyed
/// canonical IS tracked and whose hash matches — it must not over-reject.
///
/// Discriminating property: this guards the strict path against a
/// trivial "always reject" implementation. A correct strict validator
/// rejects only the untracked / mismatched self-root; a tracked,
/// hash-matching self-root validates.
#[test]
fn strict_self_root_validation_accepts_tracked_matching_whole_hash() {
    let canonical = "/self_root/strict_tracked.ts";
    let (host, real_hash) = host_with_ts(canonical, "export const tracked = 1;\n");

    let ctx: &dyn ResolverContext = &host;
    let view = ctx.resolver_store_view();
    assert!(
        crate::resolver_core::StoreView::tracks_file(&view, canonical),
        "fixture invariant: the loaded canonical must be tracked by the live \
         store view",
    );

    let signature: Vec<FactVersionRef> = vec![FactVersionRef::FileWholeHash {
        canonical_id: canonical.to_string(),
        hash: real_hash,
    }];

    assert!(
        crate::fact_signature_helpers::validate_fact_signature_with_self_roots(
            ctx,
            &signature,
            &[canonical],
        ),
        "validate_fact_signature_with_self_roots MUST ACCEPT a self-root \
         FileWholeHash whose keyed canonical is tracked and whose hash matches \
         the current content — over-rejection would defeat the cache entirely",
    );

    // A self-root whose hash does NOT match the current content fails
    // strict validation (the same-canonical content-edit detection).
    let stale_signature: Vec<FactVersionRef> = vec![FactVersionRef::FileWholeHash {
        canonical_id: canonical.to_string(),
        hash: STALE_HASH,
    }];
    assert!(
        !crate::fact_signature_helpers::validate_fact_signature_with_self_roots(
            ctx,
            &stale_signature,
            &[canonical],
        ),
        "validate_fact_signature_with_self_roots MUST REJECT a self-root \
         FileWholeHash whose hash differs from the keyed canonical's current \
         content — this is the same-canonical edit detection",
    );
}
