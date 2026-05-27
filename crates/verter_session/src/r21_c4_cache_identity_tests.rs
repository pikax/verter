//! Discriminating cache identity tests for the `from_root_body`
//! parameter on the prepared-surface walker's two query-identity
//! caches (`PreparedSurfaceDb` + `PreparedMemberDb`).
//!
//! ## Risk closed
//!
//! Without `from_root_body` in the cache key, a partial cache-incomplete
//! landing would create false confidence by exposing the
//! `declared_in_macro_type_arg` field while still serving stale or
//! defaulted provenance through one of the shared resolver paths.
//!
//! The fix threads `from_root_body` through the walker AND makes
//! it part of the cache key for both `PreparedSurfaceCacheKey` and
//! `PreparedMemberCacheKey`. Two distinct entry contexts (body
//! position vs. heritage descent) for the SAME
//! `(canonical, symbol, substitutions)` triple publish two distinct
//! cache slots whose `ProjectedSurface` / `ProjectedMember` values
//! carry the correct per-member `declared_in_macro_type_arg`.
//!
//! ## Discriminator
//!
//! The tests below construct ONE `VerterHost` and query the SAME
//! prepared decl through the walker twice — once with
//! `from_root_body=true` (the macro-T-at-body-position context), once
//! with `from_root_body=false` (the heritage-descent context). They
//! assert that:
//!
//! 1. Each query returns a `ProjectedMember`/`ProjectedSurface` with
//!    `declared_in_macro_type_arg` matching the entry context.
//! 2. Re-querying the original entry context still returns the
//!    correct value — the cache is NOT polluted by the alternate
//!    context.
//!
//! Reverting any one of the c4 production fixes (e.g. removing the
//! `from_root_body` field from `PreparedSurfaceCacheKey`, or
//! hardcoding `from_root_body=true` at a walker callsite) causes
//! these tests to fail with the discriminating fact value.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_type_expr::TypeExpr;

use crate::resolver_core::component_meta_query_engine::ComponentMetaQueryEngine;
use crate::resolver_core::ResolverContext;
use crate::{HostConfig, UpsertRequest, VerterHost};

/// Tiny upsert helper for hermetic single-file fixtures.
fn upsert(host: &VerterHost, path: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(path.to_string()),
            input_id: path.to_string(),
            source: Arc::from(source),
            file_kind: crate::FileKind::from_path(path),
            aliases: Vec::new(),
        })
        .expect("upsert succeeds");
}

/// `project_prepared_requested_member_from_symbol` — discriminating
/// cache identity for `(canonical, symbol, member, substitutions,
/// from_root_body)`.
///
/// The PreparedMemberDb cache MUST serve distinct slots for the two
/// `from_root_body` entry contexts when querying the SAME
/// `(canonical, symbol, member)`. Removing `from_root_body` from the
/// `PreparedMemberCacheKey` collapses both contexts into one slot:
/// whichever query lands first poisons the other.
///
/// Discriminating property: after this test runs, reverting any
/// of the c4 changes that gate cache identity on `from_root_body`
/// causes the body-position member to surface
/// `declared_in_macro_type_arg=false` (or vice versa), failing the
/// assertions below.
#[test]
fn prepared_member_cache_identity_discriminates_body_vs_heritage_entry_contexts() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let canonical = "/r21_c4/identity_member.ts";
    upsert(
        &host,
        canonical,
        "export interface Carrier { foo: string }\n",
    );
    host.ensure_indexed_ready(canonical)
        .expect("base IndexedReady materialises");

    let ctx: &dyn ResolverContext = &host;

    // Context 1: enter Carrier at body position
    // (`from_root_body=true`). Mirrors the top-level
    // `defineProps<Carrier>()` entry.
    let mut body_engine = ComponentMetaQueryEngine::new(ctx);
    let mut body_active: FxHashSet<(String, String)> = FxHashSet::default();
    let body_member = body_engine
        .project_prepared_requested_member_from_symbol(
            canonical,
            "Carrier",
            "foo",
            &FxHashMap::default(),
            true,
            &mut body_active,
        )
        .expect("body-context: Carrier.foo projects");
    assert!(
        body_member.declared_in_macro_type_arg,
        "body-context (from_root_body=true) — Carrier.foo MUST carry \
         declared_in_macro_type_arg=true. Got declared={}, ty={:?}. \
         A `false` here means the walker did NOT propagate `from_root_body` \
         into the single-member projection leaf.",
        body_member.declared_in_macro_type_arg, body_member.ty,
    );

    // Context 2: enter Carrier at heritage descent
    // (`from_root_body=false`). Simulates the walker recursing into
    // Carrier via an `Omit<Carrier, …>` arm.
    //
    // CRITICAL: same VerterHost, same canonical, same symbol, same
    // member — different `from_root_body`. Without `from_root_body`
    // in the cache key, this call would warm-hit the body-context
    // slot and surface `declared=true`.
    let mut heritage_engine = ComponentMetaQueryEngine::new(ctx);
    let mut heritage_active: FxHashSet<(String, String)> = FxHashSet::default();
    let heritage_member = heritage_engine
        .project_prepared_requested_member_from_symbol(
            canonical,
            "Carrier",
            "foo",
            &FxHashMap::default(),
            false,
            &mut heritage_active,
        )
        .expect("heritage-context: Carrier.foo projects");
    assert!(
        !heritage_member.declared_in_macro_type_arg,
        "heritage-context (from_root_body=false) — Carrier.foo MUST \
         carry declared_in_macro_type_arg=false. Got declared={}. \
         A `true` here means PreparedMemberDb LEAKED the body-context \
         slot (`from_root_body=true`) to the heritage-context query. \
         The cache key MUST gate on `from_root_body` per the R21 split \
         rule; reverting that field on `PreparedMemberCacheKey` causes \
         this assertion to fail.",
        heritage_member.declared_in_macro_type_arg,
    );

    // Re-query the body context: the heritage-context entry MUST NOT
    // have poisoned the cache. This catches the symmetric failure
    // (heritage query landing first, then body query warm-hitting
    // the heritage slot with `false`).
    let mut body_engine_again = ComponentMetaQueryEngine::new(ctx);
    let mut body_active_again: FxHashSet<(String, String)> = FxHashSet::default();
    let body_member_again = body_engine_again
        .project_prepared_requested_member_from_symbol(
            canonical,
            "Carrier",
            "foo",
            &FxHashMap::default(),
            true,
            &mut body_active_again,
        )
        .expect("re-body-context: Carrier.foo projects");
    assert!(
        body_member_again.declared_in_macro_type_arg,
        "re-body-context — Carrier.foo MUST STILL carry \
         declared_in_macro_type_arg=true after the heritage-context \
         query ran against the same host. Got declared={}. A `false` \
         here means the heritage-context slot poisoned the body-context \
         slot — `PreparedMemberDb` is NOT discriminating on \
         `from_root_body`.",
        body_member_again.declared_in_macro_type_arg,
    );
}

/// `cached_prepared_root_surface` — discriminating cache identity for
/// the prepared-SURFACE cache (the full-symbol surface, not a
/// single-member projection).
///
/// The PreparedSurfaceDb cache MUST serve distinct slots for the two
/// `from_root_body` entry contexts when projecting the SAME
/// `(canonical, symbol)` symbol's body. The discriminator hits every
/// member of the surface — a body-position projection stamps every
/// own-body member with `declared=true`; a heritage-context
/// projection stamps every own-body member with `declared=false`.
///
/// The test exercises the walker via `project_prepared_surface_from_symbol`
/// directly so we can set `from_root_body` explicitly (the public
/// `cached_prepared_root_surface` entry always uses `true`).
#[test]
fn prepared_surface_cache_identity_discriminates_body_vs_heritage_entry_contexts() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let canonical = "/r21_c4/identity_surface.ts";
    upsert(
        &host,
        canonical,
        "export interface Surface { alpha: string; beta: number }\n",
    );
    host.ensure_indexed_ready(canonical)
        .expect("base IndexedReady materialises");

    let ctx: &dyn ResolverContext = &host;

    // Context 1: body-position entry — every own-body member of
    // `Surface` MUST carry `declared_in_macro_type_arg=true`.
    let body_surface = {
        let mut engine = ComponentMetaQueryEngine::new(ctx);
        engine
            .cached_prepared_root_surface(canonical, "Surface")
            .expect("body-context: Surface projects")
    };
    let alpha = body_surface
        .members
        .iter()
        .find(|m| m.name == "alpha")
        .expect("body-context: alpha member present");
    let beta = body_surface
        .members
        .iter()
        .find(|m| m.name == "beta")
        .expect("body-context: beta member present");
    assert!(
        alpha.declared_in_macro_type_arg,
        "body-context — Surface.alpha MUST carry \
         declared_in_macro_type_arg=true. Got declared={}, ty={:?}.",
        alpha.declared_in_macro_type_arg, alpha.ty,
    );
    assert!(
        beta.declared_in_macro_type_arg,
        "body-context — Surface.beta MUST carry \
         declared_in_macro_type_arg=true. Got declared={}.",
        beta.declared_in_macro_type_arg,
    );

    // Context 2: heritage-descent entry — every own-body member of
    // `Surface` MUST now carry `declared_in_macro_type_arg=false`.
    // Goes through the walker's `project_prepared_surface_from_symbol`
    // with `from_root_body=false`; cached separately from the
    // body-context slot.
    let heritage_surface = {
        let mut engine = ComponentMetaQueryEngine::new(ctx);
        engine
            .r21_c4_project_prepared_surface_from_symbol_with_flag(canonical, "Surface", false)
            .expect("heritage-context: Surface projects")
    };
    let alpha_h = heritage_surface
        .members
        .iter()
        .find(|m| m.name == "alpha")
        .expect("heritage-context: alpha member present");
    let beta_h = heritage_surface
        .members
        .iter()
        .find(|m| m.name == "beta")
        .expect("heritage-context: beta member present");
    assert!(
        !alpha_h.declared_in_macro_type_arg,
        "heritage-context — Surface.alpha MUST carry \
         declared_in_macro_type_arg=false. Got declared={}. A `true` \
         here means PreparedSurfaceDb LEAKED the body-context surface \
         to the heritage-context query — the cache key is NOT \
         discriminating on `from_root_body`.",
        alpha_h.declared_in_macro_type_arg,
    );
    assert!(
        !beta_h.declared_in_macro_type_arg,
        "heritage-context — Surface.beta MUST carry \
         declared_in_macro_type_arg=false. Got declared={}.",
        beta_h.declared_in_macro_type_arg,
    );

    // Re-query the body context: the heritage-context entry MUST NOT
    // have poisoned the cache. Both body assertions MUST still hold.
    let body_surface_again = {
        let mut engine = ComponentMetaQueryEngine::new(ctx);
        engine
            .cached_prepared_root_surface(canonical, "Surface")
            .expect("re-body-context: Surface projects")
    };
    let alpha_again = body_surface_again
        .members
        .iter()
        .find(|m| m.name == "alpha")
        .expect("re-body-context: alpha member present");
    assert!(
        alpha_again.declared_in_macro_type_arg,
        "re-body-context — Surface.alpha MUST STILL carry \
         declared_in_macro_type_arg=true after the heritage-context \
         query ran against the same host. Got declared={}. A `false` \
         here means the heritage-context slot poisoned the body-context \
         slot.",
        alpha_again.declared_in_macro_type_arg,
    );
}

/// CACHE-REUSE DISCRIMINATOR: Body context query first, THEN
/// heritage context query — same VerterHost. Without
/// `from_root_body` in the cache key, the second query warm-hits
/// the first slot and serves the wrong fact.
///
/// This is the most stringent slot-discrimination test: it primes
/// the cache with the body-context entry, then queries the same
/// `(canonical, symbol)` triple at the heritage entry context, and
/// verifies the cache served the CORRECT heritage fact rather than
/// warm-hitting the body slot.
///
/// Reverts (P0 break): removing `from_root_body` from
/// `PreparedSurfaceCacheKey` or `PreparedMemberCacheKey` makes
/// the heritage query warm-hit the body slot — both assertions
/// below will then surface `declared=true` for the heritage entry.
#[test]
fn cache_reuse_discriminates_body_primed_then_heritage_query() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let canonical = "/r21_c4/cache_reuse.ts";
    upsert(
        &host,
        canonical,
        "export interface Reused { gamma: string; delta: number }\n",
    );
    host.ensure_indexed_ready(canonical)
        .expect("base IndexedReady materialises");

    let ctx: &dyn ResolverContext = &host;

    // 1. Prime the cache with the BODY-context entry. This populates
    //    the PreparedSurfaceDb slot for `(canonical, Reused,
    //    from_root_body=true)`.
    let primed_body = {
        let mut engine = ComponentMetaQueryEngine::new(ctx);
        engine
            .cached_prepared_root_surface(canonical, "Reused")
            .expect("primed body-context: Reused projects")
    };
    let primed_gamma = primed_body
        .members
        .iter()
        .find(|m| m.name == "gamma")
        .expect("primed body: gamma present");
    assert!(
        primed_gamma.declared_in_macro_type_arg,
        "Prime: body-context Reused.gamma MUST carry declared=true. \
         Got declared={}.",
        primed_gamma.declared_in_macro_type_arg,
    );

    // 2. Query the SAME `(canonical, Reused)` at HERITAGE-context.
    //    Without `from_root_body` in the cache key, this would
    //    warm-hit the body slot from step 1 and serve declared=true.
    let heritage = {
        let mut engine = ComponentMetaQueryEngine::new(ctx);
        engine
            .r21_c4_project_prepared_surface_from_symbol_with_flag(canonical, "Reused", false)
            .expect("heritage-context: Reused projects")
    };
    let heritage_gamma = heritage
        .members
        .iter()
        .find(|m| m.name == "gamma")
        .expect("heritage: gamma present");
    let heritage_delta = heritage
        .members
        .iter()
        .find(|m| m.name == "delta")
        .expect("heritage: delta present");
    assert!(
        !heritage_gamma.declared_in_macro_type_arg,
        "Heritage query after body-prime: Reused.gamma MUST carry \
         declared=false. Got declared={}. The body-context cache \
         slot LEAKED into the heritage query — `PreparedSurfaceDb` \
         is not discriminating on `from_root_body`. Removing \
         `from_root_body` from `PreparedSurfaceCacheKey` causes this \
         assertion to fail.",
        heritage_gamma.declared_in_macro_type_arg,
    );
    assert!(
        !heritage_delta.declared_in_macro_type_arg,
        "Heritage query after body-prime: Reused.delta MUST carry \
         declared=false. Got declared={}.",
        heritage_delta.declared_in_macro_type_arg,
    );
}

/// Re-export the leaf-type used by the test fixture so the
/// assertions stay typed.
#[allow(dead_code)]
type _ProjectedTypeExpr = TypeExpr;
