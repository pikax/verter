//! Request-scoped native host binding substrate.
//!
//! [`BoundNativeHostRequest`] is a sealed sum over the registered
//! framework host-integration variants (Vue, Svelte). A value witnesses
//! that ONE immutable source snapshot was bound to the EXACT registered
//! adapter × carrier-language × framework-epoch × host-epoch catalog row,
//! and it carries nothing else:
//!
//! - the opaque framework-specific host binding (the registered backend the
//!   selected compiler-backend row hands out) — reachable only through the
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
//! Framework identity is derived SOLELY from the compiler-backend
//! registry — the EXACT adapter × framework-epoch × host-epoch native-host
//! row, with the framework epoch read from the registered
//! artifact, never from path text, extension sniffing, or a lane-supplied
//! framework flag — and every unavailable outcome is a typed
//! [`NativeHostBindingUnavailable`] arm, never a fallback to another
//! framework or host.

use std::sync::Arc;

use verter_compiler::framework_common::{
    CarrierCompilerBackendRegistry, CatalogIdentity, HostEpoch, HostEpochId, HostIntegrationMiss,
    SvelteHostIntegrationBackend, VueHostIntegrationBackend,
};
use verter_identity::successor::{CarrierProfileId, FrameworkProfileId};
use verter_language::FrameworkAdapterId;
use verter_scheduler::node::{SourceSnapshot, SourceVersion};

use crate::types::{
    CompileCacheMode, CompileFailure, DiagnosticsSnapshot, HostDiagnostic, HostSeverity,
};
use crate::HostError;
use verter_session_query::analysis::types::Hash16;

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
    source_version: SourceVersion,
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

    /// Scheduler version (node incarnation and generation) the bound
    /// snapshot was committed at.
    #[must_use]
    pub fn source_version(&self) -> SourceVersion {
        self.source_version
    }
}

/// Structured catalog + snapshot identity attributing one bound request.
///
/// The catalog identity is the SAME row identity the host-integration
/// catalog registered — carrier profile, framework profile, host epoch,
/// capability — so audit attribution can never disagree with the arm that
/// was selected.
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

    /// Framework profile witness (from the registered row).
    #[must_use]
    pub fn framework_profile(&self) -> &'static FrameworkProfileId {
        self.catalog_identity.framework_profile()
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
    backend: &'static VueHostIntegrationBackend,
    attribution: NativeHostRequestAttribution,
}

impl BoundVueNativeHost {
    /// The single by-value consumption point for a Vue binding: destroys
    /// the binding and yields the catalog-registered framework-specific
    /// host binding beside the attribution. Performs no execution.
    /// Session-internal: host content is never fetchable as a service
    /// from outside the crate — the consuming execution routes live here.
    #[must_use]
    pub(crate) fn into_host_backend(
        self,
    ) -> (
        &'static VueHostIntegrationBackend,
        NativeHostRequestAttribution,
    ) {
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
    backend: &'static SvelteHostIntegrationBackend,
    attribution: NativeHostRequestAttribution,
}

impl BoundSvelteNativeHost {
    /// The single by-value consumption point for a Svelte binding:
    /// destroys the binding and yields the catalog-registered
    /// framework-specific host binding beside the attribution. Performs
    /// no execution. Session-internal: host content is never fetchable as
    /// a service from outside the crate — the consuming execution routes
    /// live here.
    #[must_use]
    pub(crate) fn into_host_backend(
        self,
    ) -> (
        &'static SvelteHostIntegrationBackend,
        NativeHostRequestAttribution,
    ) {
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
    /// The carrier has rows for the requested host epoch, but none for
    /// the artifact's framework profile.
    MismatchedFrameworkProfile {
        /// The adapter the caller asked to bind.
        adapter_id: FrameworkAdapterId,
        /// The framework profile the registered artifact carries.
        requested_framework_profile: FrameworkProfileId,
    },
    /// The supplied snapshot is no longer the live source version for its
    /// canonical file — a newer generation of the same node, or a node a
    /// removal, reset or re-home replaced — so a binding over it would
    /// attribute and consume superseded bytes.
    StaleSnapshot {
        /// Canonical file identity of the stale snapshot.
        canonical_id: Arc<str>,
        /// Version the snapshot was committed at.
        snapshot_version: SourceVersion,
        /// The live source version observed at bind time.
        live_version: SourceVersion,
    },
}

impl BoundNativeHostRequest {
    /// The ONE guarded constructor.
    ///
    /// Derives framework identity SOLELY from the compiler-backend registry:
    /// the row whose native-host registration is the EXACT `adapter_id` ×
    /// `framework_epoch` × `HostE` host-epoch triple must carry the
    /// caller's carrier language, and the backend type it was registered
    /// from chooses the variant.
    /// The snapshot must still be the live source version: same node
    /// incarnation AND same generation. A generation alone names a version
    /// only within one node object, so a snapshot held across a
    /// remove/re-add or reset that restarted at the same generation is
    /// still refused.
    ///
    /// Caller-input obligations. `adapter_id`, `carrier_language_id`, and
    /// `framework_epoch` must come from a REGISTERED identity row — the
    /// parse artifact's `adapter_id()`/`language_id()`/`epoch()` or the
    /// registered `FileLanguage` row — never from path text, extension
    /// sniffing, or a lane flag. `canonical_id`, `snapshot`, and
    /// `live_source_version` must all be read from ONE request
    /// context: the constructor cannot detect a canonical id paired with
    /// another file's snapshot, and `live_source_version` must be
    /// sourced from the scheduler/store authority at bind time, never a
    /// lane-computed value.
    ///
    /// The staleness check here is a best-effort bind-time witness: a
    /// supersession may still land between observing the live version
    /// and publishing. The durable fail-closed rail remains the
    /// publish-time completion fence, which revalidates against the
    /// carried `(canonical_id, whole_hash, source_version)` witness.
    ///
    /// Guard order is deterministic: registry identity first (unregistered
    /// carrier or adapter → host-epoch mismatch → framework-profile
    /// mismatch), then snapshot staleness, then the backend type. Two
    /// releases registered for one carrier and host epoch disambiguate by
    /// the artifact's profile; two rows for one exact key cannot exist,
    /// because the registry refuses to compose them.
    pub fn bind<HostE: HostEpoch>(
        adapter_id: &FrameworkAdapterId,
        carrier: &CarrierProfileId,
        framework_profile: &FrameworkProfileId,
        canonical_id: &str,
        snapshot: &SourceSnapshot,
        live_source_version: SourceVersion,
    ) -> Result<Self, NativeHostBindingUnavailable> {
        Self::bind_in_registry::<HostE>(
            CarrierCompilerBackendRegistry::built_in(),
            adapter_id,
            carrier,
            framework_profile,
            canonical_id,
            snapshot,
            live_source_version,
        )
    }

    /// [`Self::bind`] over an explicit registry, so a registered backend the
    /// session has no binding arm for is provable against a purpose-built
    /// snapshot. Production always binds against the built-in registry
    /// through [`Self::bind`].
    fn bind_in_registry<HostE: HostEpoch>(
        registry: CarrierCompilerBackendRegistry<'static>,
        adapter_id: &FrameworkAdapterId,
        carrier: &CarrierProfileId,
        framework_profile: &FrameworkProfileId,
        canonical_id: &str,
        snapshot: &SourceSnapshot,
        live_source_version: SourceVersion,
    ) -> Result<Self, NativeHostBindingUnavailable> {
        record_binding_construction_attempt();

        let row = registry
            .host_integration_for::<HostE>(carrier, framework_profile)
            .map_err(|miss| match miss {
                HostIntegrationMiss::UnregisteredAdapter => {
                    NativeHostBindingUnavailable::UnregisteredIdentity {
                        adapter_id: adapter_id.clone(),
                    }
                }
                HostIntegrationMiss::HostEpoch => {
                    NativeHostBindingUnavailable::MismatchedHostEpoch {
                        adapter_id: adapter_id.clone(),
                        requested_host_epoch: HostE::ID,
                    }
                }
                HostIntegrationMiss::FrameworkEpoch => {
                    NativeHostBindingUnavailable::MismatchedFrameworkProfile {
                        adapter_id: adapter_id.clone(),
                        requested_framework_profile: framework_profile.clone(),
                    }
                }
            })?;
        let identity = row.host_identity();
        if identity.adapter_id() != adapter_id {
            return Err(NativeHostBindingUnavailable::UnregisteredIdentity {
                adapter_id: adapter_id.clone(),
            });
        }
        if snapshot.version() != live_source_version {
            return Err(NativeHostBindingUnavailable::StaleSnapshot {
                canonical_id: Arc::from(canonical_id),
                snapshot_version: snapshot.version(),
                live_version: live_source_version,
            });
        }

        let attribution = NativeHostRequestAttribution {
            catalog_identity: identity,
            snapshot: BoundSourceSnapshotIdentity {
                canonical_id: Arc::from(canonical_id),
                whole_hash: snapshot.whole_hash,
                source_version: snapshot.version(),
            },
        };
        if let Some(backend) = row.native_host::<VueHostIntegrationBackend>() {
            return Ok(Self::Vue(BoundVueNativeHost {
                backend,
                attribution,
            }));
        }
        if let Some(backend) = row.native_host::<SvelteHostIntegrationBackend>() {
            return Ok(Self::Svelte(BoundSvelteNativeHost {
                backend,
                attribution,
            }));
        }
        // A registered native-host backend the session has no binding arm
        // for: the carrier registers, but nothing here can execute it.
        Err(NativeHostBindingUnavailable::UnregisteredIdentity {
            adapter_id: adapter_id.clone(),
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

impl crate::VerterHost {
    /// The ONE common production binding point for host compile attempts:
    /// every compile attempt — the host-backed `compile_entry` route and
    /// the runtime-render `compile_entry_runtime_render` route — creates
    /// exactly one [`BoundNativeHostRequest`] here, from its immutable
    /// request snapshot, and threads the binding into the route.
    /// A warm hit performs no compile and never reaches this point.
    ///
    /// Framework identity derives SOLELY from the registered parse
    /// artifact's identity row (`adapter_id()`/`language_id()`/`epoch()`)
    /// through the host-integration catalog — never from path text or
    /// language classification. `canonical_id` and the source snapshot come from
    /// the request's ONE coherent scheduler read; the live source
    /// version is re-read from the scheduler authority at bind time (a
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
            artifact.carrier_profile(),
            artifact.framework_profile(),
            canonical_id,
            source_snap,
            live.version(),
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
    use verter_compiler::framework_common::profile::carrier_profile;
    use verter_language::LanguageId;
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
            incarnation: 0,
            data: Arc::new(EmptyData),
        }
    }

    fn version(generation: u64) -> SourceVersion {
        SourceVersion {
            incarnation: 0,
            generation,
        }
    }

    /// The registered framework-profile witness for an adapter, read from
    /// the built-in compiler-backend rows themselves (never a hand-minted
    /// value).
    fn registered_profile(adapter_id: &FrameworkAdapterId) -> &'static FrameworkProfileId {
        CarrierCompilerBackendRegistry::built_in()
            .rows()
            .find(|row| row.host_identity().adapter_id() == adapter_id)
            .expect("the adapter has a registered compiler-backend row")
            .host_identity()
            .framework_profile()
    }

    fn vue_epoch() -> &'static FrameworkProfileId {
        registered_profile(&FrameworkAdapterId::vue())
    }

    fn svelte_epoch() -> &'static FrameworkProfileId {
        registered_profile(&FrameworkAdapterId::svelte())
    }

    fn bind_vue(
        snapshot: &SourceSnapshot,
        live: SourceVersion,
    ) -> Result<BoundNativeHostRequest, NativeHostBindingUnavailable> {
        BoundNativeHostRequest::bind::<NativeHostEpoch>(
            &FrameworkAdapterId::vue(),
            &carrier_profile(&VueHostIntegrationBackend::registered().carrier_language_id()),
            vue_epoch(),
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
        let bound = bind_vue(&snap, version(3)).expect("registered current Vue identity binds");
        let BoundNativeHostRequest::Vue(vue) = bound else {
            panic!("the Vue catalog arm must select the Vue variant");
        };
        let attribution = vue.attribution();
        let identity = attribution.catalog_identity();
        assert_eq!(
            identity.capability(),
            verter_compiler::framework_common::CatalogCapability::HostIntegration
        );
        assert_eq!(identity.adapter_id(), &FrameworkAdapterId::vue());
        assert_eq!(
            identity.carrier_language_id(),
            &VueHostIntegrationBackend::registered().carrier_language_id()
        );
        assert_eq!(attribution.catalog_identity().framework_family(), "vue");
        assert_eq!(
            attribution.host_epoch().map(HostEpochId::as_str),
            Some(NativeHostEpoch::ID)
        );
        let snap_id = attribution.snapshot();
        assert_eq!(snap_id.canonical_id(), CANONICAL);
        assert_eq!(snap_id.whole_hash(), &[7; 16]);
        assert_eq!(snap_id.source_version(), version(3));
    }

    /// The registered Svelte identity binds the Svelte arm: the variant
    /// is chosen by the catalog arm, so a Svelte identity can never
    /// produce a Vue binding (and vice versa).
    #[test]
    fn svelte_identity_binds_the_svelte_variant_from_the_catalog_row() {
        let snap = snapshot(1);
        let bound = BoundNativeHostRequest::bind::<NativeHostEpoch>(
            &FrameworkAdapterId::svelte(),
            &carrier_profile(&SvelteHostIntegrationBackend::registered().carrier_language_id()),
            svelte_epoch(),
            "/src/App.svelte",
            &snap,
            version(1),
        )
        .expect("registered current Svelte identity binds");
        assert!(
            matches!(bound, BoundNativeHostRequest::Svelte(_)),
            "the Svelte catalog arm must select the Svelte variant"
        );
        assert_eq!(
            bound.attribution().catalog_identity().framework_family(),
            "svelte"
        );
    }

    /// The variant comes from the registered identity alone: a Svelte
    /// adapter presented with a `.vue`-suffixed canonical path still
    /// binds the Svelte arm — path text never participates in selection.
    #[test]
    fn registered_identity_outweighs_the_canonical_paths_extension() {
        let snap = snapshot(1);
        let bound = BoundNativeHostRequest::bind::<NativeHostEpoch>(
            &FrameworkAdapterId::svelte(),
            &carrier_profile(&SvelteHostIntegrationBackend::registered().carrier_language_id()),
            svelte_epoch(),
            "/src/Confusing.vue",
            &snap,
            version(1),
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
            &carrier_profile(&VueHostIntegrationBackend::registered().carrier_language_id()),
            vue_epoch(),
            CANONICAL,
            &snap,
            version(0),
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
            &carrier_profile(&VueHostIntegrationBackend::registered().carrier_language_id()),
            vue_epoch(),
            CANONICAL,
            &snap,
            version(0),
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

    /// A registered adapter and host epoch presented with a framework
    /// epoch no row carries fails closed with the typed framework-epoch
    /// mismatch — never an arbitrary row pick, never a fallback epoch.
    #[test]
    fn mismatched_framework_epoch_fails_closed_typed() {
        let snap = snapshot(0);
        let err = BoundNativeHostRequest::bind::<NativeHostEpoch>(
            &FrameworkAdapterId::vue(),
            &carrier_profile(&VueHostIntegrationBackend::registered().carrier_language_id()),
            svelte_epoch(),
            CANONICAL,
            &snap,
            version(0),
        )
        .expect_err("a framework epoch with no registered Vue row must not bind");
        assert_eq!(
            err,
            NativeHostBindingUnavailable::MismatchedFrameworkProfile {
                adapter_id: FrameworkAdapterId::vue(),
                requested_framework_profile: svelte_epoch().clone(),
            }
        );
    }

    struct UnboundEpoch;
    impl verter_compiler::framework_common::FrameworkEpoch for UnboundEpoch {
        const MANIFEST: verter_compiler::framework_common::profile::FrameworkProfileManifest =
            verter_compiler::framework_common::profile::FrameworkProfileManifest {
                family: "unbound",
                release: "1",
                admitted_majors: &[1],
                claimed_carriers: &[],
            };
    }

    /// A runtime backend the session has no binding arm for.
    struct UnboundRuntime;
    impl verter_compiler::framework_common::RegisteredBackend for UnboundRuntime {
        fn registered() -> &'static Self {
            &UnboundRuntime
        }
    }
    impl verter_compiler::framework_common::RuntimeCompilerBackend<UnboundEpoch> for UnboundRuntime {
        type RuntimeClient = ();
        type RuntimeServer = ();
        type ParseArtifact = ();
        type Request = ();
        type ExecutionInputs = ();
        type Error = ();
        type Output = ();

        fn compile_runtime(
            &self,
            _grant: verter_compiler::framework_common::ProductExecutionGrant,
            _source: &str,
            _artifact: &(),
            _request: &(),
            _inputs: &(),
        ) -> Result<(), ()> {
            Err(())
        }
    }

    /// A native-host backend the session has no binding arm for.
    struct UnboundHost;
    impl verter_compiler::framework_common::RegisteredBackend for UnboundHost {
        fn registered() -> &'static Self {
            &UnboundHost
        }
    }
    impl
        verter_compiler::framework_common::FrameworkHostIntegrationBackend<
            UnboundEpoch,
            NativeHostEpoch,
        > for UnboundHost
    {
        type CompileAdmission = ();
        type ParseArtifact = ();
        type MultiProductDemand = ();
        type RuntimeRenderDemand = ();
        type AdmissionRefusal = ();

        fn admit_host_products(&self, _artifact: &(), _demand: ()) -> Result<(), ()> {
            Err(())
        }

        fn admit_runtime_render(&self, _artifact: &(), _demand: ()) -> Result<(), ()> {
            Err(())
        }

        fn admit_canonical_request(
            &self,
            _artifact: &(),
            _request: verter_compiler::compile_request::CompileRequest,
        ) -> Result<(), ()> {
            Err(())
        }
    }

    /// The carrier whose compiler backend the session cannot bind: a
    /// registered frontend plus a runtime and native-host backend of types
    /// no binding arm names.
    fn unbound_carrier_rows(
        rows: &mut verter_compiler::framework_common::CatalogRows,
    ) -> Result<(), verter_identity::successor::ReleaseDescriptorError> {
        use verter_compiler::framework_common::{
            Present, TypedCapabilityRegistration, VueCarrierFrontend,
        };
        let adapter = FrameworkAdapterId::new("unbound");
        let language = LanguageId::new("unbound");
        rows.carrier(TypedCapabilityRegistration::register_frontend::<
            UnboundEpoch,
            _,
        >(
            adapter.clone(),
            language.clone(),
            Present(VueCarrierFrontend),
        )?);
        rows.compiler_backend(
            TypedCapabilityRegistration::register_runtime::<UnboundEpoch, _>(
                adapter.clone(),
                language.clone(),
                Present(UnboundRuntime),
            )?,
            TypedCapabilityRegistration::register_host_integration::<
                UnboundEpoch,
                NativeHostEpoch,
                _,
            >(adapter, language, Present(UnboundHost))?,
        );
        Ok(())
    }

    /// A carrier whose compiler backend registers, but whose backend types
    /// no binding arm names, fails closed typed — the registered row is
    /// found, and the bind still never falls back to the Vue or Svelte arm.
    #[test]
    fn a_registered_host_backend_the_session_cannot_bind_fails_closed_typed() {
        use verter_compiler::framework_common::{vue_catalog_rows, CatalogSnapshot};
        let catalog: &'static CatalogSnapshot = Box::leak(Box::new(
            CatalogSnapshot::compose(&[vue_catalog_rows, unbound_carrier_rows])
                .expect("the unbound carrier composes beside Vue"),
        ));
        let adapter = FrameworkAdapterId::new("unbound");
        let epoch = catalog
            .compiler_backends()
            .rows()
            .find(|row| row.host_identity().framework_family() == "unbound")
            .map(|row| row.host_identity().framework_profile())
            .expect("the unbound carrier's row is registered");
        let snap = snapshot(1);
        let err = BoundNativeHostRequest::bind_in_registry::<NativeHostEpoch>(
            catalog.compiler_backends(),
            &adapter,
            &carrier_profile(&LanguageId::new("unbound")),
            epoch,
            "/src/App.unbound",
            &snap,
            version(1),
        )
        .expect_err("a backend type with no binding arm must not bind");
        assert_eq!(
            err,
            NativeHostBindingUnavailable::UnregisteredIdentity {
                adapter_id: adapter,
            }
        );
    }

    /// A carrier the requested framework profile does not claim fails
    /// closed — the exact carrier × profile row, not just the adapter,
    /// authorizes the bind.
    #[test]
    fn a_carrier_outside_the_requested_profile_fails_closed_typed() {
        let snap = snapshot(0);
        let err = BoundNativeHostRequest::bind::<NativeHostEpoch>(
            &FrameworkAdapterId::vue(),
            &carrier_profile(&SvelteHostIntegrationBackend::registered().carrier_language_id()),
            vue_epoch(),
            CANONICAL,
            &snap,
            version(0),
        )
        .expect_err("a carrier outside the requested profile must not bind");
        assert_eq!(
            err,
            NativeHostBindingUnavailable::MismatchedFrameworkProfile {
                adapter_id: FrameworkAdapterId::vue(),
                requested_framework_profile: vue_epoch().clone(),
            }
        );
    }

    /// A snapshot superseded by a newer live generation fails closed with
    /// the typed stale-snapshot outcome carrying both generations.
    #[test]
    fn stale_snapshot_fails_closed_typed() {
        let snap = snapshot(3);
        let err = bind_vue(&snap, version(4)).expect_err("a superseded snapshot must not bind");
        assert_eq!(
            err,
            NativeHostBindingUnavailable::StaleSnapshot {
                canonical_id: Arc::from(CANONICAL),
                snapshot_version: version(3),
                live_version: version(4),
            }
        );
    }

    /// A snapshot from a replaced node object is stale even at the same
    /// generation: a successor started by a removal or reset restarts its
    /// generation sequence, so only the incarnation tells them apart.
    #[test]
    fn same_generation_from_a_replaced_node_fails_closed_typed() {
        let snap = snapshot(1);
        let successor = SourceVersion {
            incarnation: 1,
            generation: 1,
        };
        let err = bind_vue(&snap, successor).expect_err("a replaced node's snapshot must not bind");
        assert_eq!(
            err,
            NativeHostBindingUnavailable::StaleSnapshot {
                canonical_id: Arc::from(CANONICAL),
                snapshot_version: version(1),
                live_version: successor,
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
            &carrier_profile(&VueHostIntegrationBackend::registered().carrier_language_id()),
            vue_epoch(),
            CANONICAL,
            &snap,
            version(4),
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
            .render_only_main(
                CANONICAL,
                &crate::types::CompileProfile::default(),
                verter_compiler::compile_request::RuntimeStyleProcessing::Complete,
            )
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

    /// Fail-closed across a remove/re-add: the successor node restarts its
    /// generation sequence, so a snapshot held from the removed node can
    /// share the live generation. The bind still refuses it as
    /// superseded, because the live version names a different node.
    #[test]
    fn request_snapshot_from_a_removed_node_fails_closed_as_superseded() {
        let host = crate::VerterHost::new_standalone(crate::HostConfig::default());
        upsert_vue(&host, VUE_SRC);
        let stale_snap = host
            .scheduler
            .try_get_source(CANONICAL)
            .expect("the upserted source is live");
        let efs = host
            .effective_file_state_from_snapshot(&stale_snap, CANONICAL, None)
            .expect("the upserted source carries host data");
        host.remove(CANONICAL).expect("the canonical is tracked");
        upsert_vue(
            &host,
            "<script setup lang=\"ts\">const c: number = 2</script>\n<template><div>{{ c }}</div></template>",
        );
        let live = host
            .scheduler
            .try_get_source(CANONICAL)
            .expect("the re-added source is live");
        assert_eq!(
            live.generation, stale_snap.generation,
            "the successor node reaches the removed node's generation"
        );
        assert_ne!(live.version(), stale_snap.version());
        let err = host
            .bind_native_host_compile_attempt(
                efs.framework_parse.as_deref(),
                CANONICAL,
                stale_snap.source.len() as u32,
                &stale_snap,
                crate::types::CompileCacheMode::Session,
            )
            .expect_err("a removed node's request snapshot must not bind");
        assert!(
            matches!(err, crate::HostError::Superseded),
            "a stale bind must surface as the typed Superseded host error, got: {err:?}"
        );
    }

    /// Fail-closed at the common production binding point: a registered
    /// artifact whose framework epoch has no installed host-integration
    /// row refuses the bind with the typed HOST_NATIVE_BINDING_UNAVAILABLE
    /// compile error — no fallback framework, no compile, nothing
    /// published.
    #[test]
    fn mismatched_artifact_framework_epoch_refuses_typed_and_publishes_nothing() {
        let host = crate::VerterHost::new_standalone(crate::HostConfig::default());
        upsert_vue(&host, VUE_SRC);
        let snap = host
            .scheduler
            .try_get_source(CANONICAL)
            .expect("the upserted source is live");
        let efs = host
            .effective_file_state_from_snapshot(&snap, CANONICAL, None)
            .expect("the upserted source carries host data");
        let artifact = efs
            .framework_parse
            .as_deref()
            .expect("a Vue carrier registers a framework parse artifact");
        let reminted = artifact.with_unregistered_profile_for_tests();
        let err = host
            .bind_native_host_compile_attempt(
                Some(&reminted),
                CANONICAL,
                snap.source.len() as u32,
                &snap,
                crate::types::CompileCacheMode::Session,
            )
            .expect_err("an unregistered framework epoch must not bind");
        let crate::HostError::CompileError(failure) = err else {
            panic!("expected the typed compile-error refusal, got another host error");
        };
        assert!(
            failure
                .diagnostics
                .diagnostics
                .iter()
                .any(
                    |diagnostic| diagnostic.code == "HOST_NATIVE_BINDING_UNAVAILABLE"
                        && diagnostic.message.contains("MismatchedFrameworkProfile")
                ),
            "the refusal carries the typed binding-unavailable diagnostic, got {:?}",
            failure.diagnostics.diagnostics
        );
        assert!(
            host.compile_cache()
                .get(CANONICAL)
                .is_none_or(|state| state.compile_slots.is_empty()),
            "a refused bind publishes no compile output"
        );
    }

    /// The bound arm's catalog-carried `&'static` backend reference can
    /// drive execution directly: the session consumes the binding, issues
    /// the demand-specific admission through the referenced backend, and
    /// executes it by value — no move out of the registered instance. The
    /// admission's parse key pairs the issuance with this execution, and
    /// the admission is consumed by value.
    #[test]
    fn bound_backend_reference_issues_and_executes_a_by_value_admission() {
        use verter_compiler::framework_common::{
            FrameworkHostIntegrationBackend as _, VueHostExecutionInputs,
            VueHostRuntimeRenderDemand,
        };
        let host = crate::VerterHost::new_standalone(crate::HostConfig::default());
        upsert_vue(&host, VUE_SRC);
        let snap = host
            .scheduler
            .try_get_source(CANONICAL)
            .expect("the upserted source is live");
        let efs = host
            .effective_file_state_from_snapshot(&snap, CANONICAL, None)
            .expect("the upserted source carries host data");
        let artifact = efs
            .framework_parse
            .as_deref()
            .expect("a Vue carrier registers a framework parse artifact");
        let binding = host
            .bind_native_host_compile_attempt(
                Some(artifact),
                CANONICAL,
                snap.source.len() as u32,
                &snap,
                crate::types::CompileCacheMode::Session,
            )
            .expect("the registered identity binds")
            .expect("a carrier artifact exists");
        let BoundNativeHostRequest::Vue(vue) = binding else {
            panic!("the Vue catalog arm binds the Vue variant");
        };
        let (backend, attribution) = vue.into_host_backend();
        assert_eq!(attribution.snapshot().canonical_id(), CANONICAL);
        let admission = backend
            .admit_runtime_render(artifact, VueHostRuntimeRenderDemand::default())
            .expect("the bound backend issues the render admission");
        let alloc = oxc_allocator::Allocator::new();
        let rendered = backend
            .compile_runtime_render(
                admission,
                artifact,
                &VueHostExecutionInputs::default(),
                &alloc,
            )
            .expect("the bound backend executes the admitted render");
        assert!(
            rendered.runtime_bundle().has_runtime_surface(),
            "execution through the bound reference produces the runtime Main"
        );
    }

    /// The consumption seam is by value: it destroys the binding and
    /// yields the catalog-selected backend beside the attribution,
    /// executing nothing. (That the binding is gone afterwards is
    /// enforced by move semantics; this pins what the seam yields.)
    #[test]
    fn consumption_is_by_value_and_yields_backend_plus_attribution() {
        let snap = snapshot(9);
        let bound = bind_vue(&snap, version(9)).expect("current Vue identity binds");
        let BoundNativeHostRequest::Vue(vue) = bound else {
            panic!("Vue identity selects the Vue arm");
        };
        let (_backend, attribution) = vue.into_host_backend();
        assert_eq!(attribution.snapshot().source_version(), version(9));
        assert_eq!(attribution.snapshot().canonical_id(), CANONICAL);
    }
}
