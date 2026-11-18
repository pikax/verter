//! The two carrier registries over one [`CatalogSnapshot`].
//!
//! [`CarrierFrontendRegistry`] selects the carrier frontend every registered
//! parse and projection goes through. Its rows carry an object-safe
//! [`RegisteredCarrierFrontend`] that each framework implements in its own
//! module, so the framework-neutral parse and projection callers never match
//! on a framework.
//!
//! [`CarrierCompilerBackendRegistry`] groups a carrier's runtime backend and
//! native-host integration backend into one optional row. A row carries no
//! payload: it records the backend types it was registered from, and typed
//! access ([`CarrierCompilerBackendRow::runtime`],
//! [`CarrierCompilerBackendRow::native_host`]) returns that type's own
//! registered instance only when the recorded type matches, so a lookup can
//! never fall back to another carrier's backend. A carrier without a compiler
//! backend has no row; nothing stands in for the missing backend.
//!
//! The keyed accessors are the only way to reach a row by key, and each one
//! counts as one registry dispatch ([`take_registry_dispatches`]).

use std::any::TypeId;
use std::sync::{Arc, OnceLock};

use verter_language::carrier_grammar::AcceptedRegisteredCarrierSource;
use verter_language::{
    CarrierBlockInventory, CarrierParse, FrameworkAdapterId, InventoryValidationError, LanguageId,
    ParseOptions, SyntaxReject, UnregisteredFrameworkParseArtifact,
};

use verter_identity::successor::{CarrierProfileId, FrameworkProfileId};

use super::capability::HostEpoch;
use super::capability::HostEpochId;
use super::catalog::{
    CatalogIdentity, CatalogRowProvider, CatalogSnapshot, CatalogTableRow, FrontendCap, HostCap,
    ProjectionCap, RuntimeCap, SemanticCap, TypedCapabilityRegistration,
};
use super::registered_carrier_projection::{
    catalog_miss_reject, InstalledProjectionBackend, InstalledSemanticAuthority,
};

/// A carrier frontend as the framework-neutral parse and projection routes
/// see it. Implemented beside each framework's [`super::CarrierFrontend`]
/// impl; the registry stores it type-erased on the carrier's frontend row.
pub trait RegisteredCarrierFrontend: Send + Sync + 'static {
    /// Parse `source` through this frontend. Recoverable syntax stays `Ok`
    /// with mapped diagnostics; a request the frontend cannot honor rejects
    /// before an artifact exists.
    fn parse(
        &self,
        source: &str,
        opts: &ParseOptions,
    ) -> Result<Arc<UnregisteredFrameworkParseArtifact>, SyntaxReject>;

    /// Project the registered carrier geometry of an artifact this frontend
    /// parsed from `accepted`'s bytes: the block inventory, and the erased
    /// carrier the registered artifact retains.
    fn project_geometry(
        &self,
        accepted: &AcceptedRegisteredCarrierSource,
        artifact: &Arc<UnregisteredFrameworkParseArtifact>,
    ) -> Result<(CarrierBlockInventory, Arc<dyn CarrierParse>), InventoryValidationError>;
}

/// The frontend a carrier's catalog row carries.
pub struct InstalledCarrierFrontend(Box<dyn RegisteredCarrierFrontend>);

impl InstalledCarrierFrontend {
    pub(super) fn new<F: RegisteredCarrierFrontend>(frontend: F) -> Self {
        Self(Box::new(frontend))
    }

    /// Parse `source` through the row's frontend. This is the frontend parse
    /// entry of every catalog-selected parse.
    pub fn parse(
        &self,
        source: &str,
        opts: &ParseOptions,
    ) -> Result<Arc<UnregisteredFrameworkParseArtifact>, SyntaxReject> {
        #[cfg(test)]
        super::registered_carrier_projection::record_registered_frontend_parse();
        self.0.parse(source, opts)
    }

    pub(super) fn project_geometry(
        &self,
        accepted: &AcceptedRegisteredCarrierSource,
        artifact: &Arc<UnregisteredFrameworkParseArtifact>,
    ) -> Result<(CarrierBlockInventory, Arc<dyn CarrierParse>), InventoryValidationError> {
        self.0.project_geometry(accepted, artifact)
    }
}

impl std::fmt::Debug for InstalledCarrierFrontend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InstalledCarrierFrontend")
            .finish_non_exhaustive()
    }
}

/// A carrier's frontend row: identity, registered grammar fact, and the
/// installed frontend.
pub type CarrierFrontendRow = TypedCapabilityRegistration<FrontendCap<InstalledCarrierFrontend>>;

/// A framework's semantic-authority row.
pub type SemanticAuthorityRow =
    TypedCapabilityRegistration<SemanticCap<InstalledSemanticAuthority>>;

/// A framework's projection-backend row.
pub type ProjectionBackendRow =
    TypedCapabilityRegistration<ProjectionCap<InstalledProjectionBackend>>;

/// A backend type that owns its one registered instance. Typed access to a
/// compiler-backend row hands out this instance when the row was registered
/// from the type.
pub trait RegisteredBackend: Send + Sync + 'static {
    /// The backend's registered instance.
    fn registered() -> &'static Self;
}

/// One carrier's compiler backend: its runtime backend and its native-host
/// integration backend, registered together. The row is keyed by the
/// runtime registration's identity; the host registration's identity is
/// kept for attribution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarrierCompilerBackendRow {
    runtime: CatalogIdentity,
    native_host: CatalogIdentity,
    runtime_type: TypeId,
    native_host_type: TypeId,
}

impl CarrierCompilerBackendRow {
    pub(super) fn new<R: RegisteredBackend, H: RegisteredBackend>(
        runtime: TypedCapabilityRegistration<RuntimeCap<R>>,
        native_host: TypedCapabilityRegistration<HostCap<H>>,
    ) -> Self {
        Self {
            runtime: runtime.identity().clone(),
            native_host: native_host.identity().clone(),
            runtime_type: TypeId::of::<R>(),
            native_host_type: TypeId::of::<H>(),
        }
    }

    /// The runtime registration's identity: the row's key.
    #[must_use]
    pub fn identity(&self) -> &CatalogIdentity {
        &self.runtime
    }

    /// The native-host registration's identity (capability
    /// `HostIntegration`, carrying the host epoch), for attribution.
    #[must_use]
    pub fn host_identity(&self) -> &CatalogIdentity {
        &self.native_host
    }

    /// The registered runtime backend when this row was registered from
    /// `R`; `None` for a row registered from any other runtime type.
    #[must_use]
    pub fn runtime<R: RegisteredBackend>(&self) -> Option<&'static R> {
        (self.runtime_type == TypeId::of::<R>()).then(R::registered)
    }

    /// The registered native-host backend when this row was registered from
    /// `H`; `None` for a row registered from any other host type.
    #[must_use]
    pub fn native_host<H: RegisteredBackend>(&self) -> Option<&'static H> {
        (self.native_host_type == TypeId::of::<H>()).then(H::registered)
    }
}

impl CatalogTableRow for CarrierCompilerBackendRow {
    fn identity(&self) -> &CatalogIdentity {
        &self.runtime
    }
}

/// The carrier frontend registry over one snapshot.
#[derive(Clone, Copy)]
pub struct CarrierFrontendRegistry<'s> {
    snapshot: &'s CatalogSnapshot,
}

impl CarrierFrontendRegistry<'static> {
    /// The registry over the built-in snapshot.
    #[must_use]
    pub fn built_in() -> Self {
        CatalogSnapshot::built_in().frontends()
    }
}

impl<'s> CarrierFrontendRegistry<'s> {
    pub(super) fn over(snapshot: &'s CatalogSnapshot) -> Self {
        Self { snapshot }
    }

    /// The frontend row for one adapter × carrier-language identity, or
    /// `None` when no frontend is registered for it.
    #[must_use]
    pub fn row_for(
        self,
        adapter_id: &FrameworkAdapterId,
        carrier_language_id: &LanguageId,
    ) -> Option<&'s CarrierFrontendRow> {
        record_registry_dispatch(RegistryTable::Frontend);
        self.snapshot.carriers.iter().find(|row| {
            let identity = row.identity();
            identity.adapter_id() == adapter_id
                && identity.carrier_language_id() == carrier_language_id
        })
    }

    /// Every frontend row, in catalog order. Iteration is not a dispatch.
    pub fn rows(self) -> impl Iterator<Item = &'s CarrierFrontendRow> {
        self.snapshot.carriers.iter()
    }

    /// Parse through the frontend registered for `(adapter, language)`. A
    /// miss is [`SyntaxReject::UnsupportedProfile`] (`FrontendMismatch`):
    /// never a fallback parse and never a panic.
    pub fn parse(
        self,
        adapter_id: &FrameworkAdapterId,
        carrier_language_id: &LanguageId,
        source: &str,
        opts: &ParseOptions,
    ) -> Result<Arc<UnregisteredFrameworkParseArtifact>, SyntaxReject> {
        match self.row_for(adapter_id, carrier_language_id) {
            Some(row) => row.frontend().parse(source, opts),
            None => Err(catalog_miss_reject(
                source,
                adapter_id,
                carrier_language_id,
                opts,
            )),
        }
    }
}

/// Why no native-host integration row serves a binding: the first missing
/// axis, checked in declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostIntegrationMiss {
    /// No compiler-backend row names the carrier.
    UnregisteredAdapter,
    /// The carrier has rows, but none for the requested host epoch.
    HostEpoch,
    /// The carrier has rows for the host epoch, but none for the framework
    /// profile.
    FrameworkEpoch,
}

/// The carrier compiler-backend registry over one snapshot.
#[derive(Clone, Copy)]
pub struct CarrierCompilerBackendRegistry<'s> {
    snapshot: &'s CatalogSnapshot,
}

impl CarrierCompilerBackendRegistry<'static> {
    /// The registry over the built-in snapshot.
    #[must_use]
    pub fn built_in() -> Self {
        CatalogSnapshot::built_in().compiler_backends()
    }
}

impl<'s> CarrierCompilerBackendRegistry<'s> {
    pub(super) fn over(snapshot: &'s CatalogSnapshot) -> Self {
        Self { snapshot }
    }

    /// The compiler-backend row for one carrier × framework profile, or
    /// `None` when the carrier registers no compiler backend for it.
    #[must_use]
    pub fn row_for(
        self,
        carrier: &CarrierProfileId,
        profile: &FrameworkProfileId,
    ) -> Option<&'s CarrierCompilerBackendRow> {
        record_registry_dispatch(RegistryTable::Backend);
        self.snapshot.compiler_backends.iter().find(|row| {
            let identity = row.identity();
            identity.carrier() == carrier && identity.framework_profile() == profile
        })
    }

    /// The registered runtime backend `R` for one carrier × framework profile:
    /// one [`Self::row_for`], then the row's typed access. `None` when the
    /// carrier has no compiler backend or its row was registered from
    /// another runtime type.
    #[must_use]
    pub fn runtime<R: RegisteredBackend>(
        self,
        carrier: &CarrierProfileId,
        profile: &FrameworkProfileId,
    ) -> Option<&'static R> {
        self.row_for(carrier, profile)?.runtime::<R>()
    }

    /// The compiler-backend row whose native-host registration is the
    /// exact carrier × framework profile × `HostE` host-epoch row, in one
    /// pass. A miss names the first axis no row matches.
    pub fn host_integration_for<HostE: HostEpoch>(
        self,
        carrier: &CarrierProfileId,
        profile: &FrameworkProfileId,
    ) -> Result<&'s CarrierCompilerBackendRow, HostIntegrationMiss> {
        record_registry_dispatch(RegistryTable::Backend);
        let mut adapter_registered = false;
        let mut host_epoch_matched = false;
        for row in self.snapshot.compiler_backends.iter() {
            let identity = row.host_identity();
            if identity.carrier() != carrier {
                continue;
            }
            adapter_registered = true;
            if !identity.host_epoch().is_some_and(HostEpochId::is::<HostE>) {
                continue;
            }
            host_epoch_matched = true;
            if identity.framework_profile() == profile {
                return Ok(row);
            }
        }
        Err(if !adapter_registered {
            HostIntegrationMiss::UnregisteredAdapter
        } else if !host_epoch_matched {
            HostIntegrationMiss::HostEpoch
        } else {
            HostIntegrationMiss::FrameworkEpoch
        })
    }

    /// Every compiler-backend row, in catalog order. Iteration is not a
    /// dispatch.
    pub fn rows(self) -> impl Iterator<Item = &'s CarrierCompilerBackendRow> {
        self.snapshot.compiler_backends.iter()
    }
}

/// The frameworks the built-in snapshot composes. Adding a framework is its
/// own modules plus its provider here.
const BUILT_IN_PROVIDERS: [CatalogRowProvider; 2] = [
    super::vue_host_integration::vue_catalog_rows,
    super::svelte_host_integration::svelte_catalog_rows,
];

impl CatalogSnapshot {
    /// The built-in snapshot: the Vue and Svelte rows, composed once.
    #[must_use]
    pub fn built_in() -> &'static Self {
        static SNAPSHOT: OnceLock<CatalogSnapshot> = OnceLock::new();
        SNAPSHOT.get_or_init(|| {
            Self::compose(&BUILT_IN_PROVIDERS)
                .expect("the built-in Vue and Svelte catalog rows compose")
        })
    }
}

/// Which table a keyed lookup entered.
#[derive(Clone, Copy)]
pub(super) enum RegistryTable {
    Frontend,
    Semantic,
    Projection,
    Backend,
}

/// Keyed registry lookups on one thread, per table. Test observability only.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RegistryDispatches {
    /// Frontend-row lookups.
    pub frontend: u64,
    /// Semantic-authority-row lookups.
    pub semantic: u64,
    /// Projection-backend-row lookups.
    pub projection: u64,
    /// Compiler-backend-row lookups.
    pub backend: u64,
}

// Per-thread: a test performs its demand synchronously on its own thread, so
// sibling tests in a shared process never inflate its counts.
#[cfg(any(test, feature = "test-support"))]
thread_local! {
    static REGISTRY_DISPATCHES: std::cell::Cell<RegistryDispatches> =
        const {
            std::cell::Cell::new(RegistryDispatches {
                frontend: 0,
                semantic: 0,
                projection: 0,
                backend: 0,
            })
        };
}

pub(super) fn record_registry_dispatch(table: RegistryTable) {
    #[cfg(any(test, feature = "test-support"))]
    REGISTRY_DISPATCHES.with(|cell| {
        let mut dispatches = cell.get();
        let count = match table {
            RegistryTable::Frontend => &mut dispatches.frontend,
            RegistryTable::Semantic => &mut dispatches.semantic,
            RegistryTable::Projection => &mut dispatches.projection,
            RegistryTable::Backend => &mut dispatches.backend,
        };
        *count = count.saturating_add(1);
        cell.set(dispatches);
    });
    #[cfg(not(any(test, feature = "test-support")))]
    let _ = table;
}

/// Registry dispatches on the calling thread since the last take. Test
/// observability only.
#[cfg(any(test, feature = "test-support"))]
#[must_use]
pub fn take_registry_dispatches() -> RegistryDispatches {
    REGISTRY_DISPATCHES.with(std::cell::Cell::take)
}
