//! The engine's four SHARED-CACHE producers: `ImportedRegistryDb`,
//! `DeclarationLookupDb`, `ResolvabilityDb`, `OwnerCollectionDb`.
//!
//! Inherent methods on `ComponentMetaQueryEngine<'a>`, defined in a sibling
//! `impl<'a>` block (the engine's read-through scratch mirrors and the
//! host-owned DBs they front live in the parent module).
//!
//! ## Why these four live together
//!
//! Each admits into a QUERY-IDENTITY shared cache whose entry self-roots on a
//! canonical's content hash observed OUTSIDE the value's compute. That shape
//! has exactly one hazard, and it is the reason this module exists:
//!
//! > The entry's fact stamps are read from the LIVE view, while the value can
//! > be derived from a basis the live view cannot re-check — a FENCED
//! > (`store_published == false`) `IndexedReady` serve, a BROKEN decl-body
//! > lease (`LeaseMiss`), an UNROOTABLE import route, or an UNOBSERVABLE
//! > contributor source env. Once admitted, such an entry validates on every
//! > warm read, forever: no read-side rail can reject it.
//!
//! Three of those four reasons are CONTENT-NEUTRAL — the artifact stays
//! published and content-current, so the hash does NOT move and re-resolution
//! never rescues the entry. "Safe by rooting" is therefore not an argument any
//! of these producers may rely on.
//!
//! ## The shared discipline
//!
//! Every producer here opens a CACHEABILITY TRACER SCOPE
//! ([`crate::fact_signature_helpers::with_cacheability_scope`]) as the
//! OUTERMOST bracket of its cold path and hands the scope's
//! [`CacheabilityProbe`](crate::fact_signature_helpers::CacheabilityProbe) to
//! the cache funnel, which consults it AFTER the compute returns. Two
//! consequences, both structural:
//!
//! - the probe cannot be forged and the funnels REQUIRE it, so a producer that
//!   runs its compute untraced does not compile;
//! - the pre-funnel reads that feed the entry (the observed content hash, the
//!   prepared-decl bundle read) lie INSIDE the scope, so a non-cacheable read
//!   consumed there — `owner_collection_expr`'s `observed_prepared_type_decl`
//!   is exactly this case — is seen by the post-compute verdict.
//!
//! CACHEABILITY refusal is CACHE-ONLY and value-preserving: the funnel routes
//! the computed value through `ComputeAdmission::ReturnOnly`, so the winner
//! still receives what it computed (never a fabricated `Partial`, never a
//! discard-and-re-resolve). `ReturnOnly` is deliberately NON-SHAREABLE — it
//! carries no signature carrier, so a joiner cannot view-validate it and forks
//! to cold-recompute for its own view. That is the runtime's designed
//! semantics, not a defect: what the value-preserving arm removes is the
//! WINNER's double compute.
//!
//! ## The OTHER post-compute verdict — and why it DOES re-derive
//!
//! The cacheability verdict is not the funnel's only post-compute gate. After
//! it, `revalidate_after_compute` re-checks the freshly-built entry against the
//! LIVE view. It fails when the store view MOVED under the compute — a file the
//! compute read was edited, or the project generation was reset, between its
//! first read and the publish. There the funnel returns `None` and the producer
//! RE-DERIVES; that is deliberate, and it is the one case where the winner runs
//! its resolution twice.
//!
//! The two verdicts refuse for opposite reasons and must not be conflated:
//!
//! | verdict | the value is | so the funnel |
//! |---|---|---|
//! | cacheability (`Unrooted` / the probe) | a consistent snapshot of the view it ran under, merely unrootable | returns it (`ReturnOnly`), publishes nothing |
//! | post-compute revalidation | NOT a snapshot of any view — its reads straddle the mutation | discards it; the producer re-derives against the fresh view |
//!
//! Serving a straddling value would hand the caller a torn result AND bubble the
//! superseded facts into the enclosing entry's signature (making the enclosing
//! result revalidate stale and recompute WHOLE) — strictly worse than the one
//! re-derivation. Discarding it is the completion fence's
//! retry-on-mid-flight-change. Pinned by
//! `declaration_lookup_straddling_compute_is_not_served_to_the_winner`.

use super::helpers::{
    is_builtin_name, resolve_imported_registry_symbol_with_budget, ImportedRegistrySymbolResolution,
};
use super::registry_decl::prepared_decl_authored_body_locator;
use super::{engine_fact_signature_for_exported_type, ComponentMetaQueryEngine};
use crate::component_meta_caches::ComputedEntry;

impl ComponentMetaQueryEngine<'_> {
    pub fn resolve_imported_registry_symbol(
        &mut self,
        canonical_id: &str,
        exported_name: &str,
    ) -> Option<super::ResolvedImportedRegistrySymbol> {
        let key = (canonical_id.to_string(), exported_name.to_string());
        if let Some(cached) = self.imported_registry_symbols.borrow().get(&key).cloned() {
            return cached;
        }
        // Route through the ctx-owned `ImportedRegistryDb`. The local
        // RefCell view above is non-authoritative scratch; the
        // DashMap-backed DB is the authoritative cross-request cache.
        //
        // Singleflight shape: peek the shared DB first, and on a miss run the
        // resolution INSIDE the cold-build `compute` closure that
        // `ImportedRegistryDb::get_or_compute_admit` drives through the
        // query-identity `query::lookup` split-publish path.
        // `resolve_imported_registry_symbol_with_budget` consumes the
        // wildcard-route fuse (`allow_wildcard_route()` /
        // `wildcard_route_fanout`) on the slow lane — a side-effecting,
        // per-request budget. Running it inside the cooperative flight slot is
        // what bounds that cost to ONE winner: when several requests miss the
        // same key concurrently, exactly one runs the closure and the joiners
        // block on the slot condvar and reuse its published candidate.
        let arc_key = (
            std::sync::Arc::<str>::from(canonical_id),
            std::sync::Arc::<str>::from(exported_name),
        );
        // Bind the resolver context to a local `Copy` reference so the
        // request-local view inserts before and after the cooperative
        // admission call below borrow `self` only through its `RefCell`
        // fields, never through the `ctx` field.
        let ctx = self.ctx;
        let host_db = ctx.project_type_store().imported_registry_db();
        if let Some(opt_arc) = host_db.peek(&arc_key, ctx) {
            let cached = opt_arc.as_deref().cloned();
            // Per-request audit attribution: imported-registry-symbol served
            // from a host-cache peek. Differentiate warm positive from warm
            // negative (`None`) so the audit reflects how many of the warm hits
            // were actually "this symbol is known unresolvable".
            if let Some(obs) = verter_audit::current_observer() {
                obs.record_event(verter_audit::AuditEvent::ImportedRegistryWarm);
                if cached.is_none() {
                    obs.record_event(verter_audit::AuditEvent::ImportedRegistryNegative);
                }
            }
            self.imported_registry_symbols
                .borrow_mut()
                .insert(key, cached.clone());
            return cached;
        }
        // Cross-thread singleflight rendezvous seam: when the
        // imported-registry post-peek barrier is armed for this keyed
        // canonical, block here so every contending thread is past its `peek`
        // miss before any enters cooperative admission. A no-op in production
        // and whenever the gate is unarmed.
        #[cfg(test)]
        super::await_imported_registry_post_peek_barrier_for_tests(canonical_id);

        // The DB owns the OUTERMOST cacheability scope. Its preparation closure
        // covers the lazy load and observed-hash read that feed the entry's root;
        // its compute closure covers the route walk that produces the value.
        let host_value = host_db.get_or_compute_admit(
            &arc_key,
            ctx,
            || {
            // Lazy first-time loading BEFORE the content observation: the
            // imported canonical may not have been loaded yet when this
            // registry demand is its FIRST touch (a renamed import discovered
            // through an instantiated indexed-access route reaches here without
            // any prior prop-side load). Loading must happen before
            // `authoritative_current_content_hash` and before the singleflight
            // compute so the observation and the provenance-pure fact signature
            // root on the LOADED file state — a load INSIDE the compute closure
            // mutates observable state mid-flight and post-compute revalidation
            // rejects the freshly-built entry.
            let _ = ctx.ensure_loaded(canonical_id);
            // Observe the keyed canonical's content version ONCE, before the
            // value is computed, through the view-aware
            // `authoritative_current_content_hash` oracle — under a
            // `SessionResolverContext` this resolves the overlay content hash
            // for an overlay-bearing session, so an overlay-derived entry roots
            // on the overlay version. The signature builder is provenance-pure:
            // it roots the entry's self-root on this observed hash, never a
            // current-content re-read inside the cooperative-admission closure.
            // The hash is captured HERE, before the closure, and `move`-captured
            // in, so provenance purity holds regardless of which thread wins the
            // singleflight.
            let observed_keyed_hash = ctx.authoritative_current_content_hash(canonical_id);
            // Test-only injection: simulate a concurrent request that
            // validated-and-published this key into the shared DB inside this
            // request's cold window — after the `peek` miss above, before the
            // `get_or_compute_admit` call below. `get_or_compute_admit` then
            // takes its warm-hit `validate` arm and returns the injected value
            // without running the compute closure, exactly as it would under a
            // real concurrent publish.
            #[cfg(test)]
            super::INJECT_IMPORTED_REGISTRY_CONCURRENT_PUBLISH.with(|slot| {
                if let Some(symbol) = slot.borrow().clone() {
                    if let crate::cache_runtime::SignatureAdmission::Cacheable(sig) =
                        engine_fact_signature_for_exported_type(
                            ctx,
                            canonical_id,
                            exported_name,
                            observed_keyed_hash.expect(
                                "concurrent-publish injection fixture requires an observed keyed hash",
                            ),
                        )
                    {
                        host_db.insert_for_test(
                            arc_key.clone(),
                            std::sync::Arc::new(
                                crate::component_meta_caches::ImportedRegistryEntry {
                                    value: Some(std::sync::Arc::new(symbol)),
                                    fact_dep_signature: sig.facts,
                                    // A simulated concurrent publish stamps the
                                    // live project generation, exactly as the
                                    // real cold-compute path does.
                                    validated_at_generation: ctx
                                        .project_type_store()
                                        .current_project_generation(),
                                },
                            ),
                        );
                    }
                }
            });
            // Cooperative-admission cold compute. The expensive, fuse-consuming
            // resolution runs INSIDE the `compute` closure, so it runs exactly
            // ONCE per key across all concurrent waiters.
            //
            // `get_or_compute_admit` returns `Option<Option<Arc<_>>>`:
            //
            // - `Some(cached)` — a validated value is authoritative: this
            //   request's own freshly-computed outcome (admitted, or served
            //   through the funnel's `ReturnOnly` refusal), OR an entry a
            //   CONCURRENT request published into the DB between the `peek` miss
            //   above and this call (the warm-hit `validate` arm returns it
            //   without running the closure).
            // - `None` — `compute` returned `Failed`, or post-compute
            //   revalidation rejected the freshly-built entry (a file mutated
            //   mid-compute). The request resolves to a transient miss; the next
            //   request cold-recomputes. The resolution is never re-run here.
                observed_keyed_hash
            },
            |observed_keyed_hash| {
                Self::resolve_imported_registry_symbol_admission(
                    self,
                    ctx,
                    canonical_id,
                    exported_name,
                    observed_keyed_hash,
                )
            },
        );
        Self::finish_imported_registry_lookup(self, key, host_value)
    }

    /// The cold body of [`Self::resolve_imported_registry_symbol`]: runs the
    /// single side-effecting resolution and classifies its admission.
    ///
    /// Runs entirely inside the caller's cacheability tracer scope, so a
    /// non-cacheable read consumed ANYWHERE in the route walk is observed by the
    /// funnel's post-compute verdict and downgrades the admission — this body
    /// does not need to (and must not) re-check it: the funnel is the single
    /// gate, so the rail cannot be dropped by a producer that forgets.
    fn resolve_imported_registry_symbol_admission(
        &mut self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        canonical_id: &str,
        exported_name: &str,
        observed_keyed_hash: Option<crate::types::Hash16>,
    ) -> crate::cache_runtime::singleflight::ComputeAdmission<
        Option<std::sync::Arc<super::ResolvedImportedRegistrySymbol>>,
        crate::component_meta_caches::ImportedRegistryEntry,
    > {
        #[cfg(test)]
        super::IMPORTED_REGISTRY_RESOLVE_INVOCATIONS.with(|n| n.set(n.get().saturating_add(1)));
        // Cross-thread singleflight slot-coalescing rendezvous seam: when the
        // imported-registry winner-park gate is armed for this keyed canonical,
        // the cold winner blocks here — AFTER it has claimed the in-flight slot
        // (so `claimed == true` is already published and every later arrival is
        // forced onto the joiner branch) and BEFORE it runs the fuse-consuming
        // resolution / publishes / retires the slot. The test releases the
        // winner only once it has proven every joiner has coalesced onto this
        // slot, closing the window in which a descheduled worker would form a
        // second cold winner and tick the wildcard-route fuse again. A no-op in
        // production and whenever the gate is unarmed.
        #[cfg(test)]
        super::await_imported_registry_winner_park_for_tests(canonical_id);
        // Per-request audit attribution: cold path running the expensive
        // `resolve_imported_registry_symbol_with_budget` resolution. Joiners
        // that block on this closure do NOT re-enter — so the counter reflects
        // unique cold work, not per-waiter overhead.
        if let Some(obs) = verter_audit::current_observer() {
            obs.record_event(verter_audit::AuditEvent::ImportedRegistryCold);
        }
        // Snapshot the project generation BEFORE the resolution dispatches any
        // work. The `fact_dep_signature` carrier validates only file-content
        // whole-hashes; a `ProjectGeneration` reset (tsconfig / path-alias / SDK
        // / workspace-folder change) bumps no file content, so the entry carries
        // its compute-time generation explicitly. The read-side gates reject the
        // entry once the live generation moves past this snapshot.
        let validated_at_generation = ctx.project_type_store().current_project_generation();
        // The single, side-effecting resolution: the wildcard-route fuse is
        // consumed here at most once per key.
        let resolved: Option<super::ResolvedImportedRegistrySymbol> =
            match resolve_imported_registry_symbol_with_budget(
                ctx,
                canonical_id,
                exported_name,
                || self.allow_wildcard_route(),
            ) {
                ImportedRegistrySymbolResolution::Resolved(opt) => opt,
                ImportedRegistrySymbolResolution::FuseTripped => {
                    // The wildcard route was needed but the per-request fuse was
                    // exhausted, so the symbol was NEVER looked up. This `None`
                    // is a GENUINE PARTIAL — admitting it as a warm negative
                    // would poison subsequent identical requests that DO have
                    // budget. Mark the request partial sticky so the whole
                    // component-meta result refuses to warm, and route the
                    // absent value through `ReturnOnly(None)` (NOT a cacheable
                    // negative).
                    crate::request_context::mark_request_result_partial();
                    return crate::cache_runtime::singleflight::ComputeAdmission::ReturnOnly {
                        value: None,
                        reason: crate::cache_runtime::NonAdmissionReason::PartialResult,
                    };
                }
            };
        let resolved_value = resolved.map(std::sync::Arc::new);
        #[cfg(test)]
        if super::FORCE_IMPORTED_REGISTRY_ADMISSION_REFUSAL.with(|f| f.get()) {
            // Deterministically reproduce the production admission-refusal
            // contract (`engine_fact_signature_*` returns `NonCacheable`) so the
            // discriminating test can drive the refused-admission path without
            // manufacturing a stale observed hash. The freshly-resolved value is
            // still returned to the winner via `ReturnOnly`.
            return crate::cache_runtime::singleflight::ComputeAdmission::ReturnOnly {
                value: resolved_value,
                reason: crate::cache_runtime::NonAdmissionReason::ForcedTestRefusal,
            };
        }
        let Some(observed) = observed_keyed_hash else {
            // No authoritative current content for the keyed canonical —
            // shared-cache admission is refused, but the value is still returned
            // via `ReturnOnly`. The missing current-content read means the
            // provenance could not be rooted to a self-root canonical, so a
            // cross-view joiner could never view-validate it.
            return crate::cache_runtime::singleflight::ComputeAdmission::ReturnOnly {
                value: resolved_value,
                reason: crate::cache_runtime::NonAdmissionReason::UnresolvedProvenance,
            };
        };
        match engine_fact_signature_for_exported_type(ctx, canonical_id, exported_name, observed) {
            crate::cache_runtime::SignatureAdmission::Cacheable(sig) => {
                crate::cache_runtime::singleflight::ComputeAdmission::Cacheable(
                    crate::component_meta_caches::ImportedRegistryEntry {
                        value: resolved_value,
                        fact_dep_signature: sig.facts,
                        validated_at_generation,
                    },
                )
            }
            crate::cache_runtime::SignatureAdmission::NonCacheable(reason) => {
                crate::cache_runtime::singleflight::ComputeAdmission::ReturnOnly {
                    value: resolved_value,
                    reason,
                }
            }
        }
    }

    /// Fold the `get_or_compute_admit` outcome back into the per-request memo
    /// and the audit stream — the shared tail of
    /// [`Self::resolve_imported_registry_symbol`].
    fn finish_imported_registry_lookup(
        &mut self,
        key: (String, String),
        host_value: Option<Option<std::sync::Arc<super::ResolvedImportedRegistrySymbol>>>,
    ) -> Option<super::ResolvedImportedRegistrySymbol> {
        let result = match host_value {
            Some(cached) => cached.as_deref().cloned(),
            None => None,
        };
        // Per-request audit attribution: a `None` result on the cold path
        // indicates the imported-registry-symbol resolution could not find the
        // symbol at all from the owner. The warm peek branch above handles the
        // warm-negative case separately.
        if result.is_none() {
            if let Some(obs) = verter_audit::current_observer() {
                obs.record_event(verter_audit::AuditEvent::ImportedRegistryNegative);
            }
        }
        self.imported_registry_symbols
            .borrow_mut()
            .insert(key, result.clone());
        result
    }

    /// Resolve a type declaration, cached per query.
    pub fn resolve_type_declaration(
        &mut self,
        canonical_source: &str,
        requested_name: &str,
    ) -> super::ResolvedTypeDeclaration {
        let key = (canonical_source.to_string(), requested_name.to_string());
        if let Some(cached) = self.declarations.borrow().get(&key).cloned() {
            return cached;
        }
        let arc_key = (
            std::sync::Arc::<str>::from(canonical_source),
            std::sync::Arc::<str>::from(requested_name),
        );
        let host_db = self.ctx.project_type_store().declaration_db();
        let declaration = {
            // Observe the keyed canonical's content version ONCE, before the
            // value is computed, through the view-aware
            // `authoritative_current_content_hash` oracle (overlay-correct under
            // a `SessionResolverContext`). The signature builder is
            // provenance-pure: it roots the entry's self-root on this observed
            // hash, never a current-content re-read inside the closure. The read
            // is INSIDE the scope because it feeds the entry's root.
            let prepare = || {
                self.ctx
                    .authoritative_current_content_hash(canonical_source)
            };
            // Both compute arms ride `ensure_indexed_ready_serve` (the
            // prepared-decl read and the dep-resolution fallback), so this cache
            // carries the fenced-serve / lease-miss exposure its siblings do: the
            // entry self-roots on the LIVE `observed_keyed_hash` while the value
            // can come from a served-without-publication artifact or a broken
            // decl-body lease — an entry the read-side rail cannot reject. The
            // funnel's CACHEABILITY verdict refuses the WRITE and hands the
            // computed declaration back through `ReturnOnly`, so THAT refusal
            // costs no second resolution. It is not the funnel's only
            // post-compute verdict — see the `None` arm below.
            let host_value =
                host_db.get_or_compute(&arc_key, self.ctx, prepare, |observed_keyed_hash| {
                    let computed = self
                        .resolve_direct_prepared_type_declaration(canonical_source, requested_name)
                        .unwrap_or_else(|| {
                            self.ctx
                                .resolve_type_declaration_for_dep(canonical_source, requested_name)
                        });
                    // Every arm below KEEPS the freshly-resolved declaration:
                    // returning a bare `None` would discard it and force the arm
                    // after the funnel to re-run the whole resolution, with no
                    // guarantee the second run reproduces the first.
                    let Some(observed) = observed_keyed_hash else {
                        // No observable content version for the keyed canonical —
                        // nothing to root the entry on.
                        return ComputedEntry::Unrooted(
                            computed,
                            crate::cache_runtime::NonAdmissionReason::EmptySignature,
                        );
                    };
                    match engine_fact_signature_for_exported_type(
                        self.ctx,
                        canonical_source,
                        requested_name,
                        observed,
                    ) {
                        crate::cache_runtime::SignatureAdmission::Cacheable(sig) => {
                            ComputedEntry::Rooted(computed, sig.facts)
                        }
                        crate::cache_runtime::SignatureAdmission::NonCacheable(reason) => {
                            ComputedEntry::Unrooted(computed, reason)
                        }
                    }
                });
            match host_value {
                Some(arc_decl) => arc_decl.as_ref().clone(),
                // `None` is a genuine compute failure, or a post-compute
                // REVALIDATION reject — the store view MOVED under the compute
                // (a file it read was edited, or the project generation was
                // reset, between its first read and the publish). It is NEVER
                // the cacheability refusal, which returns its value.
                //
                // The revalidation reject discards the value ON PURPOSE and
                // re-derives here. A rejected compute's reads STRADDLE the
                // mutation, so its value is a consistent snapshot of no view at
                // all; serving it would hand the caller a torn declaration and
                // bubble the superseded facts into the enclosing entry's
                // signature. Re-deriving against the fresh view is the
                // completion fence's retry-on-mid-flight-change, and it is also
                // the cheaper of the two: the alternative makes the enclosing
                // result revalidate stale and recompute WHOLE. Pinned by
                // `declaration_lookup_straddling_compute_is_not_served_to_the_winner`.
                None => self
                    .resolve_direct_prepared_type_declaration(canonical_source, requested_name)
                    .unwrap_or_else(|| {
                        self.ctx
                            .resolve_type_declaration_for_dep(canonical_source, requested_name)
                    }),
            }
        };
        self.declarations
            .borrow_mut()
            .insert(key, declaration.clone());
        declaration
    }

    /// Check if a registry ref can resolve, cached per query.
    pub fn can_resolve_registry_symbol(
        &mut self,
        owner_canonical: &str,
        exported_name: &str,
        source_hint: Option<&str>,
    ) -> bool {
        if is_builtin_name(exported_name) {
            return false;
        }
        let source_key = source_hint
            .filter(|s| !s.is_empty())
            .unwrap_or(owner_canonical);
        let key = (source_key.to_string(), exported_name.to_string());
        if let Some(cached) = self.resolvable.borrow().get(&key).copied() {
            return cached;
        }
        let arc_key = (
            std::sync::Arc::<str>::from(source_key),
            std::sync::Arc::<str>::from(exported_name),
        );
        let host_db = self.ctx.project_type_store().resolvable_db();
        let resolved = {
            // Observed once, before the value is computed and inside the scope —
            // it feeds the entry's root (see `resolve_type_declaration`).
            let prepare = || self.ctx.authoritative_current_content_hash(source_key);
            // The bool is DERIVED from the same resolution `ImportedRegistryDb`
            // caches, so it inherits the same exposure: a fenced serve or a
            // broken decl-body lease consumed inside
            // `resolve_imported_registry_symbol` derives the verdict from a
            // basis the live view cannot re-check, while the entry's signature
            // validates against that live view. The funnel's CACHEABILITY verdict
            // refuses the write and returns the computed bool.
            let host_value =
                host_db.get_or_compute(&arc_key, self.ctx, prepare, |observed_keyed_hash| {
                    let computed = if self.prepared_type_decl(source_key, exported_name).is_some() {
                        true
                    } else {
                        self.resolve_imported_registry_symbol(source_key, exported_name)
                            .is_some()
                    };
                    // If the imported-registry resolution above tripped the
                    // wildcard-route fuse (which marked the request-result
                    // completeness partial), the derived `false` is NOT an
                    // authoritative "unresolvable" verdict — the symbol was never
                    // looked up. Refuse to admit it; the caller still receives the
                    // bool so it never sees a spurious cached `false`. The
                    // `ResolvabilityDb` rail has no per-value partial flag, so it
                    // supplies the request-result completeness (one request resolves
                    // one component's meta) to the pure gate.
                    if crate::cache_runtime::refuse_result_cache_admission_if_partial(
                        crate::request_context::current_request_result_is_partial(),
                    ) {
                        return ComputedEntry::Unrooted(
                            computed,
                            crate::cache_runtime::NonAdmissionReason::PartialResult,
                        );
                    }
                    let Some(observed) = observed_keyed_hash else {
                        return ComputedEntry::Unrooted(
                            computed,
                            crate::cache_runtime::NonAdmissionReason::EmptySignature,
                        );
                    };
                    match engine_fact_signature_for_exported_type(
                        self.ctx,
                        source_key,
                        exported_name,
                        observed,
                    ) {
                        crate::cache_runtime::SignatureAdmission::Cacheable(sig) => {
                            ComputedEntry::Rooted(computed, sig.facts)
                        }
                        crate::cache_runtime::SignatureAdmission::NonCacheable(reason) => {
                            ComputedEntry::Unrooted(computed, reason)
                        }
                    }
                });
            match host_value {
                Some(value) => value,
                // A `None` host-value is a post-compute REVALIDATION reject (the
                // cacheability arms all RETURN their bool): the store view moved
                // under the compute, so the bool was derived from reads that
                // straddle the mutation. It is discarded and RE-DERIVED against
                // the fresh view — the completion fence's
                // retry-on-mid-flight-change — so the caller never sees a
                // spurious `false` and nothing torn is admitted. See
                // `resolve_type_declaration`'s `None` arm for the full rationale.
                None => {
                    if self.prepared_type_decl(source_key, exported_name).is_some() {
                        true
                    } else {
                        self.resolve_imported_registry_symbol(source_key, exported_name)
                            .is_some()
                    }
                }
            }
        };
        self.resolvable.borrow_mut().insert(key, resolved);
        resolved
    }

    /// Get the owner's collection-body LOCATOR for a name, cached per query.
    ///
    /// The value is the content-free [`AuthoredBodyLocator`] of the owner's
    /// prepared collection declaration body — never the body itself. Consumers
    /// lower it on demand through the ONE shared dispatch and read node-domain
    /// predicates off the lowered node.
    ///
    /// The cacheability scope must enclose `observed_prepared_type_decl`: that
    /// read is where a BROKEN DECL-BODY LEASE (`LeaseMiss`) is consumed, and it
    /// runs BEFORE the funnel's compute closure. A scope opened inside the
    /// closure would not see it — and the lease miss is CONTENT-NEUTRAL (the
    /// owner stays published and content-current), so the degraded `None`
    /// locator would root on the LIVE hash and validate on every warm read
    /// forever, permanently shadowing a recoverable declaration.
    ///
    /// [`AuthoredBodyLocator`]: verter_type_expr::locators::AuthoredBodyLocator
    pub fn owner_collection_expr(
        &mut self,
        owner_canonical: &str,
        name: &str,
    ) -> Option<verter_type_expr::locators::AuthoredBodyLocator> {
        if let Some(cached) = self.owner_collection_exprs.borrow().get(name).cloned() {
            return cached;
        }
        let arc_key = (
            std::sync::Arc::<str>::from(owner_canonical),
            std::sync::Arc::<str>::from(name),
        );
        let ctx = self.ctx;
        let host_db = ctx.project_type_store().owner_collection_db();
        let body = {
            // Observe the owner canonical's prepared decl AND the content
            // version it was materialised from from ONE prepared-decl bundle.
            // The cache value (the prepared decl's authored body LOCATOR) and
            // the entry's fact-signature self-root therefore root on a single,
            // provably-consistent content version — they cannot tear against a
            // racing `upsert`, and the observed hash is view-correct (the bundle
            // is fetched through the view-aware `prepared_decl_bundle`
            // accessor). This read is INSIDE the scope: it is the lease-miss
            // consumption point (see the method docs).
            let prepare = || self.observed_prepared_type_decl(owner_canonical, name);
            let host_value = host_db.get_or_compute(&arc_key, ctx, prepare, |observed| {
                // No prepared-decl bundle at all: there is no value to serve and
                // none to publish.
                let Some(observed) = observed.as_ref() else {
                    return ComputedEntry::Failed;
                };
                // The locator is minted from the SAME observed prepared/indexed
                // artifact the fact signature roots on — anti-tear by
                // construction (locator + facts publish from one observation).
                let computed = observed
                    .decl
                    .as_ref()
                    .map(|prepared| prepared_decl_authored_body_locator(prepared));
                // Root the signature on the canonical AND content version the
                // observation recorded — the value and the self-root then
                // provably agree on one content identity.
                match engine_fact_signature_for_exported_type(
                    ctx,
                    observed.canonical_id.as_str(),
                    name,
                    observed.whole_hash,
                ) {
                    crate::cache_runtime::SignatureAdmission::Cacheable(sig) => {
                        ComputedEntry::Rooted(computed, sig.facts)
                    }
                    crate::cache_runtime::SignatureAdmission::NonCacheable(reason) => {
                        ComputedEntry::Unrooted(computed, reason)
                    }
                }
            });
            match host_value {
                Some(locator) => locator,
                // No observation at all (`ComputedEntry::Failed`), or a
                // post-compute REVALIDATION reject — the store view moved under
                // the compute. The cacheability refusal is NOT here: it returns
                // its locator. Either way the locator is (re-)produced from a
                // FRESH prepared-decl read against the live view, so the caller
                // never receives one read across a mutation. See
                // `resolve_type_declaration`'s `None` arm for the full rationale.
                None => self
                    .prepared_type_decl(owner_canonical, name)
                    .map(|prepared| prepared_decl_authored_body_locator(&prepared)),
            }
        };
        self.owner_collection_exprs
            .borrow_mut()
            .insert(name.to_string(), body.clone());
        body
    }

    /// Resolve a prepared type declaration AND observe the content version it
    /// was materialised from — both sourced from the SAME prepared-decl bundle.
    ///
    /// A query-identity cache producer whose value is built from a
    /// `prepared_type_decl` read must root its fact signature on the content
    /// version the value was actually built from — never a later
    /// current-content re-read, which would let an `upsert` landing in the
    /// publish-race window admit a stale value under a fresh signature.
    ///
    /// This accessor fetches `canonical_id`'s prepared-decl bundle once through
    /// [`crate::resolver_core::ResolverContext::prepared_decl_bundle`] — which,
    /// under a `SessionResolverContext`, routes to the view-aware
    /// `prepared_decl_bundle_with_context` so an overlay-bearing session
    /// observes the overlay's bundle. The returned `decl` is the bundle's
    /// prepared decl for `symbol_name`; the returned `whole_hash` is
    /// [`crate::resolver_core::prepared_decl::PreparedTypeDeclCache::defining_content_hash`]
    /// — the `whole_hash` of the very `ShallowFileState` that bundle's prepared
    /// decls are built from. One bundle ⇒ the decl and the hash are provably
    /// the same content version (untorn against a racing `upsert`) AND the hash
    /// is view-correct (it reflects whatever view the bundle was materialised
    /// from). The producer threads this ONE observation into both the value and
    /// the provenance-pure signature builder.
    ///
    /// `None` when `canonical_id` has no prepared-decl bundle (unloaded /
    /// evicted); the producer then refuses shared-cache admission.
    ///
    /// **The `decl: None` case is NOT uniformly a legitimate absence.** It has
    /// two causes that are indistinguishable HERE and must be distinguished by
    /// the admission rail: (a) the requested symbol genuinely does not exist in
    /// the bundled canonical — a real absence, rooted on `whole_hash`, and a
    /// later declaration shifts that hash and invalidates it; (b) the symbol's
    /// body demand hit a BROKEN DECL-BODY LEASE — `PreparedDeclBundle::get`
    /// fans `LeaseMiss` and leaves its slot VACANT. Case (b) is
    /// CONTENT-NEUTRAL: the artifact stays published and content-current, so
    /// `whole_hash` does NOT move, and an entry rooted on it validates on every
    /// warm read forever — the recoverable declaration would be shadowed as a
    /// permanent absence. Rooting does not rescue it. That is why the producer's
    /// cacheability scope must ENCLOSE this call: the `LeaseMiss` fans onto the
    /// tracer here, and the funnel's post-compute verdict refuses the write.
    pub(crate) fn observed_prepared_type_decl(
        &mut self,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<super::ObservedPreparedTypeDecl> {
        let bundle = self.ctx.prepared_decl_bundle(canonical_id)?;
        let whole_hash = bundle.prepared_type_decls.defining_content_hash();
        // The bundle read runs inside its OWN cacheability scope so this
        // producer can tell a degraded `None` (a broken decl-body lease —
        // `LeaseMiss`) from an honest absence. The nested scope observes only:
        // the mark still fans out to every enclosing tracer, so the producer's
        // outer scope keeps refusing the shared-cache write.
        let (decl, non_cacheable) = crate::fact_signature_helpers::with_cacheability_scope(
            self.ctx.host_for_fact_tracer_install(),
            |_probe| bundle.prepared_type_decls.get(symbol_name),
        );
        // Mirror the bundle decl into the engine's per-request read-through
        // cache so a later `prepared_type_decl` call for the same
        // `(canonical_id, symbol_name)` hits the warm scratch entry instead of
        // re-resolving the bundle. A DEGRADED `None` is never mirrored: the
        // declaration is recoverable (the bundle's write-once slot stays
        // VACANT), and memoizing the miss would shadow it as a permanent
        // absence for the rest of this engine's scope.
        if decl.is_some() || !non_cacheable {
            self.prepared_type_decls.insert(
                (canonical_id.to_string(), symbol_name.to_string()),
                decl.clone(),
            );
        }
        Some(super::ObservedPreparedTypeDecl {
            decl,
            canonical_id: canonical_id.to_string(),
            whole_hash,
        })
    }
}
