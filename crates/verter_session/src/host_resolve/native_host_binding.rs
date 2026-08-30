//! Request-scoped native host binding substrate.
//!
//! [`BoundNativeHostRequest`] is a sealed sum over the registered
//! framework host-integration variants (Vue, Svelte). A value witnesses
//! that ONE immutable source snapshot was bound to the EXACT registered
//! adapter × carrier-language × framework-epoch × host-epoch catalog row,
//! and it carries nothing else:
//!
//! - the opaque framework-specific host binding (the catalog-selected
//!   [`InstalledHostIntegration`] arm) — reachable only through the
//!   single by-value consumption point, never through a borrowing
//!   accessor, so the binding cannot serve as a backend locator;
//! - the immutable source snapshot/revision identity the request was
//!   bound for;
//! - the framework and host epoch witnesses plus the full catalog
//!   identity, for structured request attribution.
//!
//! It is deliberately NOT a service locator: no frontend, semantic,
//! projection, runtime, store, audit-runtime, or cancellation service —
//! and no general capability bag — is carried or reachable.
//!
//! Type discipline: the binding is not `Clone`, not `Copy`, and not
//! serializable, and it is never cached or stored beyond the request; it
//! exists to be consumed exactly once, by value. The consuming methods
//! that EXECUTE a bound host live with the routes that call them; this
//! module only provides the by-value consumption seam and performs no
//! execution.
//!
//! [`BoundNativeHostRequest::bind`] is the ONE guarded constructor.
//! Framework identity is derived SOLELY from the registered
//! host-integration catalog — never from path text, extension sniffing,
//! or a lane-supplied framework flag — and every unavailable outcome is
//! a typed [`NativeHostBindingUnavailable`] arm, never a fallback to
//! another framework or host.

use std::sync::Arc;

use verter_compiler::framework_common::{
    built_in_host_integration_catalog, CatalogCapability, CatalogIdentity, FrameworkEpochId,
    HostEpoch, HostEpochId, InstalledHostIntegration, SvelteHostIntegrationBackend,
    VueHostIntegrationBackend,
};
use verter_language::{FrameworkAdapterId, LanguageId};
use verter_scheduler::node::SourceSnapshot;

use crate::types::{
    CompileCacheMode, CompileFailure, DiagnosticsSnapshot, Hash16, HostDiagnostic, HostSeverity,
};
use crate::HostError;

// The binding is request-scoped and consumed exactly once by value: it
// must never be duplicated (Clone/Copy) nor round-tripped through a
// serialized form that would let a stale binding outlive its snapshot.
static_assertions::assert_not_impl_any!(
    BoundNativeHostRequest: Clone, Copy, serde::Serialize, serde::Deserialize<'static>
);
static_assertions::assert_not_impl_any!(
    BoundVueNativeHost: Clone, Copy, serde::Serialize, serde::Deserialize<'static>
);
static_assertions::assert_not_impl_any!(
    BoundSvelteNativeHost: Clone, Copy, serde::Serialize, serde::Deserialize<'static>
);
static_assertions::assert_not_impl_any!(
    NativeHostRequestAttribution: Clone, Copy, serde::Serialize, serde::Deserialize<'static>
);

/// Immutable source snapshot/revision identity a binding was constructed
/// for. Facts only — no source bytes, no parse artifacts.
#[derive(Debug, PartialEq, Eq)]
pub struct BoundSourceSnapshotIdentity {
    canonical_id: Arc<str>,
    whole_hash: Hash16,
    source_generation: u64,
}

impl BoundSourceSnapshotIdentity {
    /// Canonical file identity of the bound snapshot.
    #[must_use]
    pub fn canonical_id(&self) -> &str {
        &self.canonical_id
    }

    /// Full-content hash of the bound snapshot.
    #[must_use]
    pub fn whole_hash(&self) -> &Hash16 {
        &self.whole_hash
    }

    /// Scheduler generation the bound snapshot was committed at.
    #[must_use]
    pub fn source_generation(&self) -> u64 {
        self.source_generation
    }
}

/// Structured catalog + snapshot identity attributing one bound request.
///
/// The catalog identity is the SAME row identity the host-integration
/// catalog registered — adapter, carrier language, framework epoch, host
/// epoch, capability — so audit attribution can never disagree with the
/// arm that was selected.
#[derive(Debug)]
pub struct NativeHostRequestAttribution {
    catalog_identity: &'static CatalogIdentity,
    snapshot: BoundSourceSnapshotIdentity,
}

impl NativeHostRequestAttribution {
    /// The registered catalog row identity the binding was derived from.
    #[must_use]
    pub fn catalog_identity(&self) -> &'static CatalogIdentity {
        self.catalog_identity
    }

    /// Framework epoch witness (from the registered row).
    #[must_use]
    pub fn framework_epoch(&self) -> &'static FrameworkEpochId {
        self.catalog_identity.epoch()
    }

    /// Host epoch witness (from the registered row; a host-integration
    /// row always carries one).
    #[must_use]
    pub fn host_epoch(&self) -> Option<&'static HostEpochId> {
        self.catalog_identity.host_epoch()
    }

    /// The immutable snapshot identity the request was bound for.
    #[must_use]
    pub fn snapshot(&self) -> &BoundSourceSnapshotIdentity {
        &self.snapshot
    }
}

/// The Vue arm of the sealed binding sum. Fields are private: the
/// framework-specific host binding is reachable only through the single
/// by-value consumption point.
#[derive(Debug)]
pub struct BoundVueNativeHost {
    backend: VueHostIntegrationBackend,
    attribution: NativeHostRequestAttribution,
}

impl BoundVueNativeHost {
    /// The single by-value consumption point for a Vue binding: destroys
    /// the binding and yields the framework-specific host binding beside
    /// the attribution. Performs no execution.
    #[must_use]
    pub fn into_host_backend(self) -> (VueHostIntegrationBackend, NativeHostRequestAttribution) {
        (self.backend, self.attribution)
    }

    /// Attribution identity (read-only; carries no services).
    #[must_use]
    pub fn attribution(&self) -> &NativeHostRequestAttribution {
        &self.attribution
    }
}

/// The Svelte arm of the sealed binding sum. Fields are private: the
/// framework-specific host binding is reachable only through the single
/// by-value consumption point.
#[derive(Debug)]
pub struct BoundSvelteNativeHost {
    backend: SvelteHostIntegrationBackend,
    attribution: NativeHostRequestAttribution,
}

impl BoundSvelteNativeHost {
    /// The single by-value consumption point for a Svelte binding:
    /// destroys the binding and yields the framework-specific host
    /// binding beside the attribution. Performs no execution.
    #[must_use]
    pub fn into_host_backend(self) -> (SvelteHostIntegrationBackend, NativeHostRequestAttribution) {
        (self.backend, self.attribution)
    }

    /// Attribution identity (read-only; carries no services).
    #[must_use]
    pub fn attribution(&self) -> &NativeHostRequestAttribution {
        &self.attribution
    }
}

/// Sealed request-scoped binding: one immutable source snapshot bound to
/// exactly one registered framework host-integration row.
///
/// The variant is chosen by the CATALOG arm, never by the caller, so a
/// cross-framework mismatch (a Svelte identity inside the Vue variant) is
/// structurally impossible. Variant payload fields are private, so the
/// sum cannot be forged outside this module.
#[derive(Debug)]
pub enum BoundNativeHostRequest {
    /// The registered Vue host-integration arm.
    Vue(BoundVueNativeHost),
    /// The registered Svelte host-integration arm.
    Svelte(BoundSvelteNativeHost),
}

/// Typed binding-unavailable outcomes. Every arm is fail-closed: no
/// framework fallback, no lane retry, no partially-bound value.
#[derive(Debug, PartialEq, Eq)]
pub enum NativeHostBindingUnavailable {
    /// No host-integration catalog row exists for the adapter at all.
    UnregisteredIdentity {
        /// The adapter the caller asked to bind.
        adapter_id: FrameworkAdapterId,
    },
    /// The adapter has host-integration rows, but none for the requested
    /// host epoch.
    MismatchedHostEpoch {
        /// The adapter the caller asked to bind.
        adapter_id: FrameworkAdapterId,
        /// The host epoch the caller requested.
        requested_host_epoch: &'static str,
    },
    /// More than one registered row (distinct framework epochs) matches
    /// the adapter + host epoch; selection would be arbitrary, so the
    /// bind fails closed instead of picking one.
    AmbiguousRegistration {
        /// The adapter whose registration is ambiguous.
        adapter_id: FrameworkAdapterId,
        /// The host epoch the caller requested.
        requested_host_epoch: &'static str,
    },
    /// The registered row exists, but its carrier language is not the
    /// carrier language the caller is binding for.
    CarrierLanguageMismatch {
        /// The carrier language on the registered row.
        registered: LanguageId,
        /// The carrier language the caller supplied.
        requested: LanguageId,
    },
    /// The supplied snapshot is no longer the live source generation for
    /// its canonical file; a binding over it would attribute and consume
    /// superseded bytes.
    StaleSnapshot {
        /// Canonical file identity of the stale snapshot.
        canonical_id: Arc<str>,
        /// Generation the snapshot was committed at.
        snapshot_generation: u64,
        /// The live source generation observed at bind time.
        live_generation: u64,
    },
}

impl BoundNativeHostRequest {
    /// The ONE guarded constructor.
    ///
    /// Derives framework identity SOLELY from the registered
    /// host-integration catalog: the row selected by `adapter_id` and the
    /// `HostE` host epoch chooses the variant, supplies the framework
    /// epoch witness, and must carry the caller's carrier language. The
    /// snapshot witness must still be the live source generation.
    ///
    /// Caller-input obligations. `adapter_id` and `carrier_language_id`
    /// must come from a REGISTERED identity row — the parse artifact's
    /// `adapter_id()`/`language_id()` or the registered `FileLanguage`
    /// row — never from path text, extension sniffing, or a lane flag.
    /// `canonical_id`, `snapshot`, and `live_source_generation` must all
    /// be read from ONE request context: the constructor cannot detect a
    /// canonical id paired with another file's snapshot, and
    /// `live_source_generation` must be sourced from the scheduler/store
    /// authority at bind time, never a lane-computed value.
    ///
    /// The staleness check here is a best-effort bind-time witness: a
    /// supersession may still land between observing the live generation
    /// and publishing. The durable fail-closed rail remains the
    /// publish-time completion fence, which revalidates against the
    /// carried `(canonical_id, whole_hash, source_generation)` witness.
    ///
    /// Guard order is deterministic: catalog identity first
    /// (unregistered → host-epoch mismatch → registration ambiguity, the
    /// last detected while scanning epoch-matched rows → carrier
    /// language), then snapshot staleness. A future second FRAMEWORK
    /// epoch registered for one adapter and host epoch fails closed as
    /// ambiguous; disambiguating it would add an epoch input here.
    pub fn bind<HostE: HostEpoch>(
        adapter_id: &FrameworkAdapterId,
        carrier_language_id: &LanguageId,
        canonical_id: &str,
        snapshot: &SourceSnapshot,
        live_source_generation: u64,
    ) -> Result<Self, NativeHostBindingUnavailable> {
        record_binding_construction_attempt();

        let mut adapter_registered = false;
        let mut selected: Option<(&'static CatalogIdentity, &'static InstalledHostIntegration)> =
            None;
        let mut ambiguous = false;
        for row in built_in_host_integration_catalog().iter() {
            let identity = row.identity();
            if identity.capability() != CatalogCapability::HostIntegration
                || identity.adapter_id() != adapter_id
            {
                continue;
            }
            adapter_registered = true;
            if !identity
                .host_epoch()
                .is_some_and(|host| host.as_str() == HostE::ID)
            {
                continue;
            }
            let Some(installed) = row.host_integration() else {
                continue;
            };
            if selected.is_some() {
                ambiguous = true;
                break;
            }
            selected = Some((identity, installed));
        }

        if ambiguous {
            return Err(NativeHostBindingUnavailable::AmbiguousRegistration {
                adapter_id: adapter_id.clone(),
                requested_host_epoch: HostE::ID,
            });
        }
        let Some((identity, installed)) = selected else {
            return Err(if adapter_registered {
                NativeHostBindingUnavailable::MismatchedHostEpoch {
                    adapter_id: adapter_id.clone(),
                    requested_host_epoch: HostE::ID,
                }
            } else {
                NativeHostBindingUnavailable::UnregisteredIdentity {
                    adapter_id: adapter_id.clone(),
                }
            });
        };
        if identity.carrier_language_id() != carrier_language_id {
            return Err(NativeHostBindingUnavailable::CarrierLanguageMismatch {
                registered: identity.carrier_language_id().clone(),
                requested: carrier_language_id.clone(),
            });
        }
        if snapshot.generation != live_source_generation {
            return Err(NativeHostBindingUnavailable::StaleSnapshot {
                canonical_id: Arc::from(canonical_id),
                snapshot_generation: snapshot.generation,
                live_generation: live_source_generation,
            });
        }

        let attribution = NativeHostRequestAttribution {
            catalog_identity: identity,
            snapshot: BoundSourceSnapshotIdentity {
                canonical_id: Arc::from(canonical_id),
                whole_hash: snapshot.whole_hash,
                source_generation: snapshot.generation,
            },
        };
        Ok(match installed {
            InstalledHostIntegration::Vue(backend) => Self::Vue(BoundVueNativeHost {
                backend: *backend,
                attribution,
            }),
            InstalledHostIntegration::Svelte(backend) => Self::Svelte(BoundSvelteNativeHost {
                backend: *backend,
                attribution,
            }),
        })
    }

    /// Attribution identity of either arm (read-only; carries no
    /// services and never exposes the host binding).
    #[must_use]
    pub fn attribution(&self) -> &NativeHostRequestAttribution {
        match self {
            Self::Vue(bound) => bound.attribution(),
            Self::Svelte(bound) => bound.attribution(),
        }
    }
}

impl BoundNativeHostRequest {
    /// Consumes the binding by value for a retained compatibility route:
    /// yields the attribution and drops the bound backend arm UNEXECUTED —
    /// execution stays with the route's registry-dispatched
    /// `compile_bundle` call; a route that executes through its bound
    /// backend consumes the backend itself instead of this seam. Both
    /// arms route through their single per-arm consumption points.
    #[must_use]
    pub(crate) fn into_attribution(self) -> NativeHostRequestAttribution {
        match self {
            Self::Vue(vue) => vue.into_host_backend().1,
            Self::Svelte(svelte) => svelte.into_host_backend().1,
        }
    }
}

impl crate::VerterHost {
    /// The ONE common production binding point for host compile attempts:
    /// every compile attempt — host-backed `compile_entry` and the
    /// runtime-render `compile_entry_runtime_render` compatibility route —
    /// creates exactly one [`BoundNativeHostRequest`] here, from its
    /// immutable request snapshot, and threads the binding into the route.
    /// A warm hit performs no compile and never reaches this point.
    ///
    /// Framework identity derives SOLELY from the registered parse
    /// artifact's identity row (`adapter_id()`/`language_id()`) through the
    /// host-integration catalog — never from path text or language
    /// classification. `canonical_id` and the source snapshot come from
    /// the request's ONE coherent scheduler read; the live source
    /// generation is re-read from the scheduler authority at bind time (a
    /// best-effort staleness witness — the durable rail stays the
    /// publish-time completion fence).
    ///
    /// `Ok(None)` means the input has no carrier parse artifact, so no
    /// registered identity exists to bind; the routes' own
    /// no-carrier-artifact arm reports that characterized typed refusal.
    /// Every bind failure is fail-closed and typed: a superseded snapshot
    /// maps to [`HostError::Superseded`], a vanished live source to
    /// [`HostError::MissingSource`], and a catalog identity failure to a
    /// [`HostError::CompileError`] carrying the
    /// `HOST_NATIVE_BINDING_UNAVAILABLE` diagnostic — never a fallback
    /// framework, never a silent skip, and nothing is published.
    pub(crate) fn bind_native_host_compile_attempt(
        &self,
        artifact: Option<&verter_compiler::framework_common::FrameworkParseArtifact>,
        canonical_id: &str,
        source_len: u32,
        source_snap: &verter_scheduler::node::SourceSnapshot,
        requested_mode: CompileCacheMode,
    ) -> Result<Option<BoundNativeHostRequest>, HostError> {
        use verter_compiler::framework_common::NativeHostEpoch;

        let Some(artifact) = artifact else {
            return Ok(None);
        };
        let Some(live) = self.scheduler.try_get_source(canonical_id) else {
            return Err(HostError::MissingSource {
                canonical_id: canonical_id.to_string(),
            });
        };
        match BoundNativeHostRequest::bind::<NativeHostEpoch>(
            artifact.adapter_id(),
            artifact.language_id(),
            canonical_id,
            source_snap,
            live.generation,
        ) {
            Ok(bound) => Ok(Some(bound)),
            Err(NativeHostBindingUnavailable::StaleSnapshot { .. }) => Err(HostError::Superseded),
            Err(unavailable) => Err(HostError::CompileError(CompileFailure {
                diagnostics: DiagnosticsSnapshot::from_vec(vec![HostDiagnostic {
                    severity: HostSeverity::Error,
                    code: "HOST_NATIVE_BINDING_UNAVAILABLE".to_string(),
                    message: format!(
                        "native host binding unavailable for '{canonical_id}': {unavailable:?}"
                    ),
                    arguments: Vec::new(),
                    span: verter_span::Span::new(0, source_len),
                }]),
                requested_mode,
                actual_mode: requested_mode,
                downgrade_reason: None,
            })),
        }
    }
}

#[cfg(test)]
static BINDING_CONSTRUCTION_ATTEMPTS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Process-wide count of constructor entries, so tests can pin the
/// per-attempt binding cardinality of the production compile routes.
#[cfg(test)]
pub(crate) fn binding_construction_attempts() -> usize {
    BINDING_CONSTRUCTION_ATTEMPTS.load(std::sync::atomic::Ordering::SeqCst)
}

fn record_binding_construction_attempt() {
    #[cfg(test)]
    BINDING_CONSTRUCTION_ATTEMPTS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_scheduler::node::EmptyData;

    struct OtherHostEpoch;
    impl HostEpoch for OtherHostEpoch {
        const ID: &'static str = "other-host";
    }

    use verter_compiler::framework_common::NativeHostEpoch;

    const CANONICAL: &str = "/src/App.vue";

    fn snapshot(generation: u64) -> SourceSnapshot {
        SourceSnapshot {
            source: Arc::from("<template><div /></template>"),
            whole_hash: [7; 16],
            semantic_hash: [8; 16],
            generation,
            data: Arc::new(EmptyData),
        }
    }

    fn bind_vue(
        snapshot: &SourceSnapshot,
        live: u64,
    ) -> Result<BoundNativeHostRequest, NativeHostBindingUnavailable> {
        BoundNativeHostRequest::bind::<NativeHostEpoch>(
            &FrameworkAdapterId::vue(),
            &VueHostIntegrationBackend.carrier_language_id(),
            CANONICAL,
            snapshot,
            live,
        )
    }

    /// The registered Vue identity binds the Vue arm, and the attribution
    /// is the registered catalog row identity plus the exact snapshot
    /// identity — nothing derived from path text or a caller flag.
    #[test]
    fn vue_identity_binds_the_vue_variant_from_the_catalog_row() {
        let snap = snapshot(3);
        let bound = bind_vue(&snap, 3).expect("registered current Vue identity binds");
        let BoundNativeHostRequest::Vue(vue) = bound else {
            panic!("the Vue catalog arm must select the Vue variant");
        };
        let attribution = vue.attribution();
        let identity = attribution.catalog_identity();
        assert_eq!(identity.capability(), CatalogCapability::HostIntegration);
        assert_eq!(identity.adapter_id(), &FrameworkAdapterId::vue());
        assert_eq!(
            identity.carrier_language_id(),
            &VueHostIntegrationBackend.carrier_language_id()
        );
        assert_eq!(attribution.framework_epoch().as_str(), "vue");
        assert_eq!(
            attribution.host_epoch().map(HostEpochId::as_str),
            Some(NativeHostEpoch::ID)
        );
        let snap_id = attribution.snapshot();
        assert_eq!(snap_id.canonical_id(), CANONICAL);
        assert_eq!(snap_id.whole_hash(), &[7; 16]);
        assert_eq!(snap_id.source_generation(), 3);
    }

    /// The registered Svelte identity binds the Svelte arm: the variant
    /// is chosen by the catalog arm, so a Svelte identity can never
    /// produce a Vue binding (and vice versa).
    #[test]
    fn svelte_identity_binds_the_svelte_variant_from_the_catalog_row() {
        let snap = snapshot(1);
        let bound = BoundNativeHostRequest::bind::<NativeHostEpoch>(
            &FrameworkAdapterId::svelte(),
            &SvelteHostIntegrationBackend.carrier_language_id(),
            "/src/App.svelte",
            &snap,
            1,
        )
        .expect("registered current Svelte identity binds");
        assert!(
            matches!(bound, BoundNativeHostRequest::Svelte(_)),
            "the Svelte catalog arm must select the Svelte variant"
        );
        assert_eq!(bound.attribution().framework_epoch().as_str(), "svelte");
    }

    /// The variant comes from the registered identity alone: a Svelte
    /// adapter presented with a `.vue`-suffixed canonical path still
    /// binds the Svelte arm — path text never participates in selection.
    #[test]
    fn registered_identity_outweighs_the_canonical_paths_extension() {
        let snap = snapshot(1);
        let bound = BoundNativeHostRequest::bind::<NativeHostEpoch>(
            &FrameworkAdapterId::svelte(),
            &SvelteHostIntegrationBackend.carrier_language_id(),
            "/src/Confusing.vue",
            &snap,
            1,
        )
        .expect("the registered Svelte identity binds regardless of path text");
        assert!(
            matches!(bound, BoundNativeHostRequest::Svelte(_)),
            "path extension must not influence variant selection"
        );
    }

    /// An adapter with no host-integration registration fails closed with
    /// the typed unregistered-identity outcome — no fallback framework.
    #[test]
    fn unregistered_adapter_fails_closed_typed() {
        let snap = snapshot(0);
        let unregistered = FrameworkAdapterId::new("angular");
        let err = BoundNativeHostRequest::bind::<NativeHostEpoch>(
            &unregistered,
            &VueHostIntegrationBackend.carrier_language_id(),
            CANONICAL,
            &snap,
            0,
        )
        .expect_err("an unregistered adapter must not bind");
        assert_eq!(
            err,
            NativeHostBindingUnavailable::UnregisteredIdentity {
                adapter_id: unregistered,
            }
        );
    }

    /// A registered adapter requested under a host epoch it has no row
    /// for fails closed with the typed mismatched-epoch outcome.
    #[test]
    fn mismatched_host_epoch_fails_closed_typed() {
        let snap = snapshot(0);
        let err = BoundNativeHostRequest::bind::<OtherHostEpoch>(
            &FrameworkAdapterId::vue(),
            &VueHostIntegrationBackend.carrier_language_id(),
            CANONICAL,
            &snap,
            0,
        )
        .expect_err("a host epoch with no registered row must not bind");
        assert_eq!(
            err,
            NativeHostBindingUnavailable::MismatchedHostEpoch {
                adapter_id: FrameworkAdapterId::vue(),
                requested_host_epoch: "other-host",
            }
        );
    }

    /// A registered row whose carrier language disagrees with the
    /// caller's carrier language fails closed — the exact catalog row,
    /// not just the adapter, authorizes the bind.
    #[test]
    fn carrier_language_mismatch_fails_closed_typed() {
        let snap = snapshot(0);
        let err = BoundNativeHostRequest::bind::<NativeHostEpoch>(
            &FrameworkAdapterId::vue(),
            &SvelteHostIntegrationBackend.carrier_language_id(),
            CANONICAL,
            &snap,
            0,
        )
        .expect_err("a carrier-language mismatch must not bind");
        assert_eq!(
            err,
            NativeHostBindingUnavailable::CarrierLanguageMismatch {
                registered: VueHostIntegrationBackend.carrier_language_id(),
                requested: SvelteHostIntegrationBackend.carrier_language_id(),
            }
        );
    }

    /// A snapshot superseded by a newer live generation fails closed with
    /// the typed stale-snapshot outcome carrying both generations.
    #[test]
    fn stale_snapshot_fails_closed_typed() {
        let snap = snapshot(3);
        let err = bind_vue(&snap, 4).expect_err("a superseded snapshot must not bind");
        assert_eq!(
            err,
            NativeHostBindingUnavailable::StaleSnapshot {
                canonical_id: Arc::from(CANONICAL),
                snapshot_generation: 3,
                live_generation: 4,
            }
        );
    }

    /// Identity guards run before the staleness guard: an unregistered
    /// adapter over a stale snapshot reports the identity failure, so the
    /// outcome taxonomy is deterministic and never input-order dependent.
    #[test]
    fn identity_guards_precede_the_staleness_guard() {
        let snap = snapshot(3);
        let unregistered = FrameworkAdapterId::new("angular");
        let err = BoundNativeHostRequest::bind::<NativeHostEpoch>(
            &unregistered,
            &VueHostIntegrationBackend.carrier_language_id(),
            CANONICAL,
            &snap,
            4,
        )
        .expect_err("neither guard admits this bind");
        assert!(matches!(
            err,
            NativeHostBindingUnavailable::UnregisteredIdentity { .. }
        ));
    }

    fn upsert_vue(host: &crate::VerterHost, source: &str) {
        let _ = host
            .upsert(crate::UpsertRequest {
                canonical_id: None,
                input_id: CANONICAL.to_string(),
                source: Arc::from(source),
                file_language: crate::types::FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .expect("upsert must succeed");
    }

    const VUE_SRC: &str = "<script setup lang=\"ts\">const a: number = 1</script>\n<template><div>{{ a }}</div></template>";

    /// Per-route binding cardinality on the host-backed compile route: a
    /// COLD compile attempt creates exactly ONE binding through the common
    /// binding point, and a WARM hit — which performs no compile — creates
    /// none. The counter is process-wide and this holds under the
    /// canonical per-test-process runner (nextest), where no sibling test
    /// can increment it concurrently.
    #[test]
    fn cold_compile_binds_exactly_once_and_warm_hit_binds_none() {
        let host = crate::VerterHost::new_standalone(crate::HostConfig::default());
        upsert_vue(&host, VUE_SRC);
        host.ensure_compiled(CANONICAL, &crate::types::CompileProfile::default())
            .expect("the production compile route serves this component");
        assert_eq!(
            super::binding_construction_attempts(),
            1,
            "a cold host-backed compile attempt must bind exactly once"
        );
        host.ensure_compiled(CANONICAL, &crate::types::CompileProfile::default())
            .expect("the warm serve of the same component succeeds");
        assert_eq!(
            super::binding_construction_attempts(),
            1,
            "a warm hit performs no compile and must create no binding"
        );
    }

    /// A supersession-driven re-snapshot binds ANEW for its own attempt:
    /// re-upserting new content and compiling again is a second compile
    /// attempt with its own binding over its own immutable snapshot.
    #[test]
    fn supersession_re_snapshot_binds_anew() {
        let host = crate::VerterHost::new_standalone(crate::HostConfig::default());
        upsert_vue(&host, VUE_SRC);
        host.ensure_compiled(CANONICAL, &crate::types::CompileProfile::default())
            .expect("the first revision compiles");
        assert_eq!(super::binding_construction_attempts(), 1);
        upsert_vue(
            &host,
            "<script setup lang=\"ts\">const b: string = 'x'</script>\n<template><div>{{ b }}</div></template>",
        );
        host.ensure_compiled(CANONICAL, &crate::types::CompileProfile::default())
            .expect("the superseding revision compiles");
        assert_eq!(
            super::binding_construction_attempts(),
            2,
            "a superseding re-snapshot must bind anew for its own attempt"
        );
    }

    /// Per-route binding cardinality on the runtime-render compatibility
    /// route: every render is a compile attempt (the lane has no warm
    /// serve) and binds exactly once through the same common binding
    /// point.
    #[test]
    fn render_route_binds_exactly_once() {
        let host = crate::VerterHost::new_standalone(crate::HostConfig::default());
        upsert_vue(&host, VUE_SRC);
        let render = host
            .render_only_main(CANONICAL, &crate::types::CompileProfile::default())
            .expect("the render route serves this component");
        assert!(
            !render.code.is_empty(),
            "the render route must produce the runtime Main module"
        );
        assert_eq!(
            super::binding_construction_attempts(),
            1,
            "a render compile attempt must bind exactly once"
        );
    }

    /// Fail-closed: a request snapshot superseded by a newer live source
    /// generation refuses the bind at the common binding point with the
    /// typed [`crate::HostError::Superseded`] outcome — no fallback, no
    /// compile, nothing published.
    #[test]
    fn stale_request_snapshot_fails_closed_as_superseded() {
        let host = crate::VerterHost::new_standalone(crate::HostConfig::default());
        upsert_vue(&host, VUE_SRC);
        let stale_snap = host
            .scheduler
            .try_get_source(CANONICAL)
            .expect("the upserted source is live");
        let efs = host
            .effective_file_state_from_snapshot(&stale_snap, CANONICAL, None)
            .expect("the upserted source carries host data");
        assert!(
            efs.framework_parse.is_some(),
            "a Vue carrier registers a framework parse artifact"
        );
        // Supersede the captured snapshot with a new live revision.
        upsert_vue(
            &host,
            "<script setup lang=\"ts\">const c: number = 2</script>\n<template><div>{{ c }}</div></template>",
        );
        let err = host
            .bind_native_host_compile_attempt(
                efs.framework_parse.as_deref(),
                CANONICAL,
                stale_snap.source.len() as u32,
                &stale_snap,
                crate::types::CompileCacheMode::Session,
            )
            .expect_err("a superseded request snapshot must not bind");
        assert!(
            matches!(err, crate::HostError::Superseded),
            "a stale bind must surface as the typed Superseded host error, got: {err:?}"
        );
        assert!(
            host.compile_cache()
                .get(CANONICAL)
                .is_none_or(|state| state.compile_slots.is_empty()),
            "a refused bind publishes no compile output"
        );
    }

    /// The consumption seam is by value: it destroys the binding and
    /// yields the catalog-selected backend beside the attribution,
    /// executing nothing. (That the binding is gone afterwards is
    /// enforced by move semantics; this pins what the seam yields.)
    #[test]
    fn consumption_is_by_value_and_yields_backend_plus_attribution() {
        let snap = snapshot(9);
        let bound = bind_vue(&snap, 9).expect("current Vue identity binds");
        let BoundNativeHostRequest::Vue(vue) = bound else {
            panic!("Vue identity selects the Vue arm");
        };
        let (_backend, attribution) = vue.into_host_backend();
        assert_eq!(attribution.snapshot().source_generation(), 9);
        assert_eq!(attribution.snapshot().canonical_id(), CANONICAL);
    }
}
