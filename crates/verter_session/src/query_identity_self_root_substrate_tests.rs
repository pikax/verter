//! Shared substrate for query-identity self-version rooting —
//! discriminator tests.
//!
//! Codex diagnosis: the query-identity caches do not detect a
//! **same-canonical content edit** on the resolver's cold-recompute
//! read path. A cache entry keyed by `(canonical, ...)` survives an
//! edit to `canonical` itself because its `read_set_signature` carries
//! no self-version root for the keyed canonical, and the lazy
//! "unknown means okay" validation behavior accepts an untracked
//! whole-hash fact. The own-canonical drain in `host_upsert.rs` masks
//! this gap in production by eagerly evicting the upserted canonical's
//! caches.
//!
//! This module locks down the four substrate deliverables that make
//! the own-canonical drain deletable. Each test DISCRIMINATES — it
//! fails against the pre-substrate tree and passes against the
//! post-substrate tree — and asserts a concrete observable.
//!
//! 1. [`current_file_facts_reads_current_content_not_get_any`] —
//!    `ResolverContext::current_file_facts` reads the file's parse
//!    facts by full **current** content identity and returns `None`
//!    for a stale-only artifact. Pre-substrate the producer read
//!    `FileArtifactStore::get_artifacts_any`, which returns the stale
//!    `FileArtifacts` regardless of content hash.
//! 2. [`exported_type_signature_carries_self_root_file_whole_hash`] /
//!    [`canonical_member_signature_carries_self_root_file_whole_hash`] /
//!    [`canonical_surface_signature_carries_self_root_file_whole_hash`]
//!    — the three central fact-signature helpers prepend a
//!    `FileWholeHash` self-root for their defining/key canonical.
//!    Pre-substrate the signature carried only `Parse` facts and no
//!    self-root.
//! 3. [`strict_self_root_validation_rejects_untracked_whole_hash`] —
//!    `validate_fact_signature_with_self_roots` validates a self-root
//!    `FileWholeHash` strictly: an untracked keyed canonical fails
//!    validation. The lazy `validate_fact_signature` accepts the same
//!    untracked fact.
//! 4. [`skip_own_canonical_drain_hook_leaves_caches_undrained`] — the
//!    test-only upsert hook runs the upsert pipeline but skips the
//!    own-canonical drain, so a `project_type_store` entry for the
//!    upserted canonical survives the hooked upsert where it would not
//!    survive the production `upsert`.

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
            file_kind: crate::FileKind::from_path(path),
            aliases: Vec::new(),
        })
        .expect("seed upsert succeeds");
    let indexed = host
        .ensure_indexed_ready(path)
        .expect("IndexedReady must materialise for an upserted file");
    (host, indexed.whole_hash)
}

// ---------------------------------------------------------------------------
// Item 1 — current-content fact-signature path.
// ---------------------------------------------------------------------------

/// `ResolverContext::current_file_facts` resolves the authoritative
/// current content hash and reads the file's `FileArtifacts` pinned to
/// that identity. When the only cached artifact is a stale candidate
/// for an older content hash, it returns `None`.
///
/// Discriminating property: a real artifact is materialised, then a
/// synthetic STALE `FileArtifacts` (doctored content hash) is planted
/// as the sole stored entry while the scheduler still reports the real
/// hash. A pre-substrate producer reads `get_artifacts_any`, which
/// returns the planted stale `FileArtifacts` regardless of content
/// hash — so `current_file_facts` would return `Some(<stale facts>)`.
/// Post-substrate it pins on the authoritative current hash, misses
/// the stale candidate, and returns `None`.
#[test]
fn current_file_facts_reads_current_content_not_get_any() {
    let canonical = "/self_root/probe_facts.ts";
    let (host, real_hash) = host_with_ts(
        canonical,
        "export interface Probe { a: number; }\nexport const probe = 1;\n",
    );
    assert_ne!(
        real_hash, STALE_HASH,
        "fixture invariant: the real content hash must differ from the planted stale hash",
    );

    // Before planting: the current-content read HITS the genuine
    // artifact. This anchors the discriminator so the post-plant
    // assertion is not vacuously satisfied by a systematically-missing
    // read.
    {
        let ctx: &dyn ResolverContext = &host;
        assert!(
            ctx.current_file_facts(canonical).is_some(),
            "fixture invariant: current_file_facts must HIT the genuine \
             current artifact before the stale plant",
        );
    }

    // Plant a synthetic STALE FileArtifacts as the SOLE stored entry.
    // Clone the real artifact, doctor its `IndexedReady.whole_hash` to
    // the stale sentinel, then drop every real-keyed entry and store
    // only the stale-keyed one. `FileArtifactStore::get_artifacts` is
    // content-pinned, so afterwards a current-content read pinned on
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
    // entry — this is the pre-substrate producer read shape.
    let permissive = host
        .project_type_store()
        .indexed()
        .get_artifacts_any(canonical)
        .expect("get_artifacts_any must still return the (stale) entry");
    assert_eq!(
        permissive.indexed.whole_hash, STALE_HASH,
        "fixture invariant: get_artifacts_any returns the planted stale entry — \
         that is exactly the read shape the pre-substrate producer used",
    );

    // The discriminating assertion: the current-content read pins on
    // the authoritative real hash and finds NO artifact there (only
    // the stale candidate is stored), so it returns `None`. A pre-fix
    // `get_artifacts_any` read returns `Some(<stale facts>)`.
    let ctx: &dyn ResolverContext = &host;
    assert!(
        ctx.current_file_facts(canonical).is_none(),
        "current_file_facts MUST return None when the only cached artifact is a \
         stale candidate ({STALE_HASH:?}) while the authoritative current content \
         hash is the real value ({real_hash:?}). A non-None result means the read \
         fell back to the permissive get_artifacts_any path.",
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

/// `fact_signature_for_exported_type` prepends a `FileWholeHash`
/// self-root for the defining canonical.
///
/// Discriminating property: pre-substrate the helper emitted only
/// `Parse` facts (`Export` / `LocalDecl` / `MemberShape`) and the
/// returned signature contained no `FileWholeHash` entry; this
/// assertion fails. Post-substrate the helper prepends the
/// current-content `FileWholeHash` for `canonical`.
#[test]
fn exported_type_signature_carries_self_root_file_whole_hash() {
    let canonical = "/self_root/exported_type.ts";
    let (host, real_hash) = host_with_ts(
        canonical,
        "export interface Surface { a: number; b: string; }\n",
    );

    let ctx: &dyn ResolverContext = &host;
    let signature = crate::fact_signature_helpers::fact_signature_for_exported_type(
        ctx,
        canonical,
        "Surface",
        SymbolSpace::Type,
    );

    assert!(
        has_self_root(&signature, canonical, real_hash),
        "fact_signature_for_exported_type MUST prepend a FileWholeHash self-root \
         for the defining canonical ({canonical} @ {real_hash:?}). \
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
}

/// `fact_signature_for_canonical_member` prepends a `FileWholeHash`
/// self-root for the canonical the member is declared in.
///
/// Discriminating property: pre-substrate the helper emitted only the
/// `MemberPresence` + `Member` parse facts and no `FileWholeHash`;
/// this assertion fails.
#[test]
fn canonical_member_signature_carries_self_root_file_whole_hash() {
    let canonical = "/self_root/member_owner.ts";
    let (host, real_hash) = host_with_ts(
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
    );

    assert!(
        has_self_root(&signature, canonical, real_hash),
        "fact_signature_for_canonical_member MUST prepend a FileWholeHash \
         self-root for the declaring canonical ({canonical} @ {real_hash:?}). \
         Signature was: {signature:?}",
    );
    assert!(
        signature.iter().any(|f| matches!(
            f,
            FactVersionRef::Parse(p) if matches!(p.key, FactKey::Member { .. })
        )),
        "the Member parse fact must still be present alongside the self-root",
    );
}

/// `fact_signature_for_canonical_surface` prepends a `FileWholeHash`
/// self-root for the canonical whose surface it observes.
///
/// Discriminating property: pre-substrate the helper emitted only the
/// `SyntacticExportSet` parse fact and no `FileWholeHash`; this
/// assertion fails.
#[test]
fn canonical_surface_signature_carries_self_root_file_whole_hash() {
    let canonical = "/self_root/surface_owner.ts";
    let (host, real_hash) = host_with_ts(
        canonical,
        "export const a = 1;\nexport const b = 2;\nexport type C = string;\n",
    );

    let ctx: &dyn ResolverContext = &host;
    let signature =
        crate::fact_signature_helpers::fact_signature_for_canonical_surface(ctx, canonical);

    assert!(
        has_self_root(&signature, canonical, real_hash),
        "fact_signature_for_canonical_surface MUST prepend a FileWholeHash \
         self-root for the keyed canonical ({canonical} @ {real_hash:?}). \
         Signature was: {signature:?}",
    );
    assert!(
        signature
            .iter()
            .any(|f| matches!(f, FactVersionRef::Parse(p) if p.key == FactKey::SyntacticExportSet)),
        "the SyntacticExportSet parse fact must still be present alongside the self-root",
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

// ---------------------------------------------------------------------------
// Item 4 — the temporary skip-own-drain test hook.
// ---------------------------------------------------------------------------

/// The skip-own-drain upsert hook runs the upsert pipeline but does NOT
/// drain the upserted canonical's own query-identity caches.
///
/// Discriminating property: a `project_type_store` entry for the
/// upserted canonical is seeded, then the file is re-upserted through
/// the hook. The production `upsert` calls
/// `project_type_store.evict_canonical(&canonical)` (the own-canonical
/// drain), which removes that entry. The hook skips that call, so the
/// seeded entry survives the hooked upsert. The companion
/// production-path assertion confirms the same entry does NOT survive a
/// normal `upsert` — proving the hook genuinely changes drain behavior
/// rather than being a no-op.
#[test]
fn skip_own_canonical_drain_hook_leaves_caches_undrained() {
    let canonical = "/self_root/hook_probe.vue";
    let source_v1 =
        "<script setup lang=\"ts\">\nconst a = 1;\n</script>\n<template><div/></template>\n";
    let source_v2 =
        "<script setup lang=\"ts\">\nconst a = 2;\n</script>\n<template><div/></template>\n";

    // --- Production path: the own-canonical drain removes the entry. ---
    {
        let host = VerterHost::new_standalone(HostConfig::default());
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(canonical.to_string()),
                input_id: canonical.to_string(),
                source: Arc::from(source_v1),
                file_kind: crate::FileKind::from_path(canonical),
                aliases: Vec::new(),
            })
            .expect("seed upsert succeeds");
        // Materialise an IndexedReady so project_type_store holds an
        // own-canonical entry.
        host.ensure_indexed_ready(canonical)
            .expect("IndexedReady materialises");
        assert!(
            host.project_type_store()
                .indexed()
                .get_artifacts_any(canonical)
                .is_some(),
            "fixture invariant: project_type_store must hold an own-canonical \
             entry before the re-upsert",
        );
        // Re-upsert through the PRODUCTION path — the own-canonical
        // drain runs.
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(canonical.to_string()),
                input_id: canonical.to_string(),
                source: Arc::from(source_v2),
                file_kind: crate::FileKind::from_path(canonical),
                aliases: Vec::new(),
            })
            .expect("re-upsert succeeds");
        assert!(
            host.project_type_store()
                .indexed()
                .get_artifacts_any(canonical)
                .is_none(),
            "production upsert MUST drain the upserted canonical's own \
             project_type_store entry (the own-canonical drain) — if this entry \
             survives, the production drain did not run and the hook test below \
             is not discriminating",
        );
    }

    // --- Hooked path: the own-canonical drain is skipped. ---
    {
        let host = VerterHost::new_standalone(HostConfig::default());
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(canonical.to_string()),
                input_id: canonical.to_string(),
                source: Arc::from(source_v1),
                file_kind: crate::FileKind::from_path(canonical),
                aliases: Vec::new(),
            })
            .expect("seed upsert succeeds");
        host.ensure_indexed_ready(canonical)
            .expect("IndexedReady materialises");
        assert!(
            host.project_type_store()
                .indexed()
                .get_artifacts_any(canonical)
                .is_some(),
            "fixture invariant: project_type_store must hold an own-canonical \
             entry before the hooked re-upsert",
        );
        // Re-upsert through the HOOK — the own-canonical drain is
        // skipped.
        let _ = host
            .upsert_skipping_own_canonical_drain_for_tests(UpsertRequest {
                canonical_id: Some(canonical.to_string()),
                input_id: canonical.to_string(),
                source: Arc::from(source_v2),
                file_kind: crate::FileKind::from_path(canonical),
                aliases: Vec::new(),
            })
            .expect("hooked re-upsert succeeds");

        // The discriminating assertion: the own-canonical entry SURVIVES
        // the hooked upsert because the drain was skipped. Through the
        // production `upsert` (above) the same entry is gone.
        assert!(
            host.project_type_store()
                .indexed()
                .get_artifacts_any(canonical)
                .is_some(),
            "the skip-own-drain hook MUST leave the upserted canonical's own \
             project_type_store entry intact — a missing entry means the hook \
             still ran the own-canonical drain and is not the discriminator \
             scaffold the later query-identity-cache phases need",
        );
    }
}
