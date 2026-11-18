//! Immutable compiler capability catalog.
//!
//! Uniqueness key: adapter × epoch × capability (host epoch is additional
//! uniqueness for host-integration rows). Language is row data, not a
//! uniqueness axis. Built once from typed registrations; no insert,
//! replace, or unload after construction. Capability is minted by the
//! matching `register_*` constructor and cannot disagree with the payload.
//!
//! [`CatalogSnapshot`] is the one immutable catalog: separately typed
//! tables for carrier frontends, semantic authorities, projection backends
//! and carrier compiler backends, with no row type spanning them. The
//! registries in [`super::carrier_registry`] read it.

use std::collections::HashSet;

use verter_language::carrier_grammar::CarrierGrammarConfig;
use verter_language::{FrameworkAdapterId, LanguageId};

use verter_identity::successor::{CarrierProfileId, FrameworkProfileId, ReleaseDescriptorError};

use super::capability::{
    CarrierFrontend, FrameworkEpoch, FrameworkHostIntegrationBackend, FrameworkSemanticAuthority,
    HostEpoch, HostEpochId, Present, ProjectionBackend, RuntimeCompilerBackend,
};
use super::carrier_registry::{
    record_registry_dispatch, CarrierCompilerBackendRegistry, CarrierCompilerBackendRow,
    CarrierFrontendRegistry, CarrierFrontendRow, InstalledCarrierFrontend, ProjectionBackendRow,
    RegisteredBackend, RegisteredCarrierFrontend, RegistryTable, SemanticAuthorityRow,
};
use super::profile::{carrier_profile, framework_profile};
use super::registered_carrier_projection::{
    InstalledProjectionBackend, InstalledSemanticAuthority,
};
use crate::compile_request::ProductKind;

/// Which of the five authorities a catalog row names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CatalogCapability {
    /// [`CarrierFrontend`].
    Frontend,
    /// [`FrameworkSemanticAuthority`].
    Semantic,
    /// [`ProjectionBackend`].
    Projection,
    /// [`RuntimeCompilerBackend`].
    Runtime,
    /// [`FrameworkHostIntegrationBackend`].
    HostIntegration,
}

/// Process-lifetime identity of one catalog row.
///
/// The row's identity is its carrier profile, its framework profile, its
/// host epoch (host-integration rows only) and its capability. The adapter id
/// and carrier language are the classifier entry and display data; the
/// framework family is a display spelling only.
#[derive(Debug, Clone)]
pub struct CatalogIdentity {
    carrier: CarrierProfileId,
    framework_profile: FrameworkProfileId,
    host_epoch: Option<HostEpochId>,
    capability: CatalogCapability,
    adapter_id: FrameworkAdapterId,
    carrier_language_id: LanguageId,
    framework_family: &'static str,
}

impl PartialEq for CatalogIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.order_key() == other.order_key()
    }
}

impl Eq for CatalogIdentity {}

impl std::hash::Hash for CatalogIdentity {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.order_key().hash(state);
    }
}

impl PartialOrd for CatalogIdentity {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CatalogIdentity {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.order_key().cmp(&other.order_key())
    }
}

impl CatalogIdentity {
    fn ordinary<E: FrameworkEpoch>(
        adapter_id: FrameworkAdapterId,
        carrier_language_id: LanguageId,
        capability: CatalogCapability,
    ) -> Result<Self, ReleaseDescriptorError> {
        verter_debug_assert_ne!(capability, CatalogCapability::HostIntegration);
        Ok(Self {
            carrier: carrier_profile(&carrier_language_id),
            framework_profile: framework_profile(&E::MANIFEST)?,
            host_epoch: None,
            capability,
            adapter_id,
            carrier_language_id,
            framework_family: E::MANIFEST.family,
        })
    }

    fn host_integration<E: FrameworkEpoch, HostE: HostEpoch>(
        adapter_id: FrameworkAdapterId,
        carrier_language_id: LanguageId,
    ) -> Result<Self, ReleaseDescriptorError> {
        Ok(Self {
            carrier: carrier_profile(&carrier_language_id),
            framework_profile: framework_profile(&E::MANIFEST)?,
            host_epoch: Some(HostEpochId::from_type::<HostE>()),
            capability: CatalogCapability::HostIntegration,
            adapter_id,
            carrier_language_id,
            framework_family: E::MANIFEST.family,
        })
    }

    /// Adapter identity: the classifier entry, not part of row identity.
    #[must_use]
    pub fn adapter_id(&self) -> &FrameworkAdapterId {
        &self.adapter_id
    }

    /// Carrier language: the classifier entry, not part of row identity.
    #[must_use]
    pub fn carrier_language_id(&self) -> &LanguageId {
        &self.carrier_language_id
    }

    /// The carrier profile of the row.
    #[must_use]
    pub fn carrier(&self) -> &CarrierProfileId {
        &self.carrier
    }

    /// The framework profile of the row.
    #[must_use]
    pub fn framework_profile(&self) -> &FrameworkProfileId {
        &self.framework_profile
    }

    /// The framework family the profile belongs to. Display only.
    #[must_use]
    pub fn framework_family(&self) -> &'static str {
        self.framework_family
    }

    /// Host epoch when this row is host integration; otherwise `None`.
    #[must_use]
    pub fn host_epoch(&self) -> Option<&HostEpochId> {
        self.host_epoch.as_ref()
    }

    /// Capability named by this row.
    #[must_use]
    pub fn capability(&self) -> CatalogCapability {
        self.capability
    }

    fn order_key(
        &self,
    ) -> (
        &CarrierProfileId,
        &FrameworkProfileId,
        Option<HostEpochId>,
        CatalogCapability,
    ) {
        (
            &self.carrier,
            &self.framework_profile,
            self.host_epoch,
            self.capability,
        )
    }

    fn key(&self) -> CatalogKey<'_> {
        CatalogKey {
            adapter_id: self.adapter_id.as_str(),
            framework_profile: &self.framework_profile,
            capability: self.capability,
            host_epoch: self.host_epoch,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct CatalogKey<'a> {
    adapter_id: &'a str,
    framework_profile: &'a FrameworkProfileId,
    capability: CatalogCapability,
    host_epoch: Option<HostEpochId>,
}

/// Duplicate adapter × framework profile × capability (× host epoch)
/// identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateCatalogIdentity {
    /// The identity that collided.
    pub identity: Box<CatalogIdentity>,
}

/// A row of one catalog table: the identity the table keys and orders on.
pub trait CatalogTableRow {
    /// Identity of this row.
    fn identity(&self) -> &CatalogIdentity;
}

impl<Cap> CatalogTableRow for TypedCapabilityRegistration<Cap> {
    fn identity(&self) -> &CatalogIdentity {
        &self.identity
    }
}

/// One immutable catalog table: rows of one type, unique by identity key
/// and sorted by identity. Built once; no insert, replace, or unload after.
#[derive(Debug, Clone)]
pub struct CatalogTable<Row> {
    rows: Box<[Row]>,
}

impl<Row: CatalogTableRow> CatalogTable<Row> {
    /// Build a frozen table from typed rows. Duplicate keys fail.
    pub fn try_from_rows<I>(rows: I) -> Result<Self, DuplicateCatalogIdentity>
    where
        I: IntoIterator<Item = Row>,
    {
        let mut rows: Vec<Row> = rows.into_iter().collect();
        let mut seen = HashSet::with_capacity(rows.len());
        for row in &rows {
            if !seen.insert(row.identity().key()) {
                return Err(DuplicateCatalogIdentity {
                    identity: Box::new(row.identity().clone()),
                });
            }
        }
        rows.sort_by(|a, b| a.identity().cmp(b.identity()));
        Ok(Self {
            rows: rows.into_boxed_slice(),
        })
    }
}

impl<Row> CatalogTable<Row> {
    /// Deterministic row iteration (sorted by adapter, language, epoch, host
    /// epoch, capability).
    pub fn iter(&self) -> std::slice::Iter<'_, Row> {
        self.rows.iter()
    }

    /// Number of frozen rows.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the table is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// Typed registration of exactly one present capability.
///
/// Accessors for other capabilities are not implemented on the corresponding
/// constructor's type, so a frontend-only row cannot name a runtime backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedCapabilityRegistration<Cap> {
    identity: CatalogIdentity,
    capability: Cap,
}

impl TypedCapabilityRegistration<()> {
    /// Frontend-only registration. No runtime backend is in the type.
    /// Capability is minted as [`CatalogCapability::Frontend`].
    ///
    /// Catalog epoch identity is derived from `E`. The frontend trait has
    /// no epoch type parameter; the constructor still takes `E` so there
    /// is no independent epoch-value argument.
    pub fn register_frontend<E: FrameworkEpoch, F: CarrierFrontend>(
        adapter_id: FrameworkAdapterId,
        carrier_language_id: LanguageId,
        frontend: Present<F>,
    ) -> Result<TypedCapabilityRegistration<FrontendCap<F>>, ReleaseDescriptorError> {
        Ok(TypedCapabilityRegistration {
            identity: CatalogIdentity::ordinary::<E>(
                adapter_id,
                carrier_language_id,
                CatalogCapability::Frontend,
            )?,
            capability: FrontendCap {
                frontend,
                grammar: None,
            },
        })
    }

    /// Projection-only registration. No runtime backend is in the type.
    ///
    /// Catalog epoch identity is derived from `E`. The projection trait
    /// has no epoch type parameter; the constructor still takes `E`.
    pub fn register_projection<E: FrameworkEpoch, P: ProjectionBackend>(
        adapter_id: FrameworkAdapterId,
        carrier_language_id: LanguageId,
        projection: Present<P>,
    ) -> Result<TypedCapabilityRegistration<ProjectionCap<P>>, ReleaseDescriptorError> {
        Ok(TypedCapabilityRegistration {
            identity: CatalogIdentity::ordinary::<E>(
                adapter_id,
                carrier_language_id,
                CatalogCapability::Projection,
            )?,
            capability: ProjectionCap(projection),
        })
    }

    /// Semantic-authority registration.
    ///
    /// Catalog epoch identity is derived from `E`. There is no separate
    /// epoch-value argument that could disagree with the authority type.
    pub fn register_semantic<E: FrameworkEpoch, S: FrameworkSemanticAuthority<E>>(
        adapter_id: FrameworkAdapterId,
        carrier_language_id: LanguageId,
        semantic: Present<S>,
    ) -> Result<TypedCapabilityRegistration<SemanticCap<S>>, ReleaseDescriptorError> {
        Ok(TypedCapabilityRegistration {
            identity: CatalogIdentity::ordinary::<E>(
                adapter_id,
                carrier_language_id,
                CatalogCapability::Semantic,
            )?,
            capability: SemanticCap(semantic),
        })
    }

    /// Runtime-capable registration with a real backend (not a stub).
    ///
    /// Catalog epoch identity is derived from `E`.
    pub fn register_runtime<E: FrameworkEpoch, R: RuntimeCompilerBackend<E>>(
        adapter_id: FrameworkAdapterId,
        carrier_language_id: LanguageId,
        runtime: Present<R>,
    ) -> Result<TypedCapabilityRegistration<RuntimeCap<R>>, ReleaseDescriptorError> {
        Ok(TypedCapabilityRegistration {
            identity: CatalogIdentity::ordinary::<E>(
                adapter_id,
                carrier_language_id,
                CatalogCapability::Runtime,
            )?,
            capability: RuntimeCap(runtime),
        })
    }

    /// Host-integration registration. Framework and host epochs are
    /// derived from `E` and `HostE`.
    pub fn register_host_integration<
        E: FrameworkEpoch,
        HostE: HostEpoch,
        H: FrameworkHostIntegrationBackend<E, HostE>,
    >(
        adapter_id: FrameworkAdapterId,
        carrier_language_id: LanguageId,
        host: Present<H>,
    ) -> Result<TypedCapabilityRegistration<HostCap<H>>, ReleaseDescriptorError> {
        Ok(TypedCapabilityRegistration {
            identity: CatalogIdentity::host_integration::<E, HostE>(
                adapter_id,
                carrier_language_id,
            )?,
            capability: HostCap(host),
        })
    }
}

/// Frontend capability payload: the present frontend plus the row's
/// registered carrier-grammar fact, when one is registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontendCap<F> {
    frontend: Present<F>,
    grammar: Option<CarrierGrammarConfig>,
}
/// Projection capability payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionCap<P>(Present<P>);
/// Semantic capability payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticCap<S>(Present<S>);
/// Runtime capability payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeCap<R>(Present<R>);
/// Host-integration capability payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostCap<H>(Present<H>);

impl<F> TypedCapabilityRegistration<FrontendCap<F>> {
    /// Row identity.
    #[must_use]
    pub fn identity(&self) -> &CatalogIdentity {
        &self.identity
    }

    /// The present frontend.
    #[must_use]
    pub fn frontend(&self) -> &F {
        &self.capability.frontend.0
    }

    /// Attach this row's registered carrier-grammar fact.
    #[must_use]
    pub fn with_registered_grammar(mut self, grammar: CarrierGrammarConfig) -> Self {
        self.capability.grammar = Some(grammar);
        self
    }

    /// The row's registered carrier-grammar fact, when one is registered.
    #[must_use]
    pub fn registered_grammar(&self) -> Option<&CarrierGrammarConfig> {
        self.capability.grammar.as_ref()
    }

    /// Re-wrap the present frontend, keeping this row's identity and
    /// registered grammar fact.
    #[must_use]
    pub fn map_frontend<G>(
        self,
        map: impl FnOnce(F) -> G,
    ) -> TypedCapabilityRegistration<FrontendCap<G>> {
        TypedCapabilityRegistration {
            identity: self.identity,
            capability: FrontendCap {
                frontend: Present(map(self.capability.frontend.0)),
                grammar: self.capability.grammar,
            },
        }
    }
}

impl<P> TypedCapabilityRegistration<ProjectionCap<P>> {
    /// Row identity.
    #[must_use]
    pub fn identity(&self) -> &CatalogIdentity {
        &self.identity
    }

    /// The present projection backend.
    #[must_use]
    pub fn projection(&self) -> &P {
        &self.capability.0 .0
    }

    /// Re-wrap the present projection backend, keeping this row's identity.
    #[must_use]
    pub fn map_projection<Q>(
        self,
        map: impl FnOnce(P) -> Q,
    ) -> TypedCapabilityRegistration<ProjectionCap<Q>> {
        TypedCapabilityRegistration {
            identity: self.identity,
            capability: ProjectionCap(Present(map(self.capability.0 .0))),
        }
    }
}

impl<S> TypedCapabilityRegistration<SemanticCap<S>> {
    /// Row identity.
    #[must_use]
    pub fn identity(&self) -> &CatalogIdentity {
        &self.identity
    }

    /// The present semantic authority.
    #[must_use]
    pub fn semantic(&self) -> &S {
        &self.capability.0 .0
    }

    /// Re-wrap the present semantic authority, keeping this row's identity.
    #[must_use]
    pub fn map_semantic<T>(
        self,
        map: impl FnOnce(S) -> T,
    ) -> TypedCapabilityRegistration<SemanticCap<T>> {
        TypedCapabilityRegistration {
            identity: self.identity,
            capability: SemanticCap(Present(map(self.capability.0 .0))),
        }
    }
}

impl<R> TypedCapabilityRegistration<RuntimeCap<R>> {
    /// Row identity.
    #[must_use]
    pub fn identity(&self) -> &CatalogIdentity {
        &self.identity
    }

    /// The present runtime backend.
    #[must_use]
    pub fn runtime(&self) -> &R {
        &self.capability.0 .0
    }
}

impl<H> TypedCapabilityRegistration<HostCap<H>> {
    /// Row identity.
    #[must_use]
    pub fn identity(&self) -> &CatalogIdentity {
        &self.identity
    }

    /// The present host-integration backend.
    #[must_use]
    pub fn host_integration(&self) -> &H {
        &self.capability.0 .0
    }
}

/// The rows one provider contributes to a [`CatalogSnapshot`]. Each
/// framework registers its rows from its own module through the typed
/// registration constructors; [`CatalogSnapshot::compose`] freezes them.
#[derive(Default)]
pub struct CatalogRows {
    carriers: Vec<CarrierFrontendRow>,
    semantic: Vec<SemanticAuthorityRow>,
    projection: Vec<ProjectionBackendRow>,
    compiler_backends: Vec<CarrierCompilerBackendRow>,
}

impl CatalogRows {
    /// A carrier's frontend row.
    pub fn carrier<F: RegisteredCarrierFrontend>(
        &mut self,
        row: TypedCapabilityRegistration<FrontendCap<F>>,
    ) {
        self.carriers
            .push(row.map_frontend(InstalledCarrierFrontend::new));
    }

    /// A framework's semantic-authority row.
    pub fn semantic(&mut self, row: SemanticAuthorityRow) {
        self.semantic.push(row);
    }

    /// A framework's projection-backend row.
    pub fn projection(&mut self, row: ProjectionBackendRow) {
        self.projection.push(row);
    }

    /// A carrier's compiler backend: its runtime and native-host
    /// registrations as one row. Only the identities and the backend types
    /// are kept; typed access hands out each type's registered instance.
    pub fn compiler_backend<R: RegisteredBackend, H: RegisteredBackend>(
        &mut self,
        runtime: TypedCapabilityRegistration<RuntimeCap<R>>,
        native_host: TypedCapabilityRegistration<HostCap<H>>,
    ) {
        self.compiler_backends
            .push(CarrierCompilerBackendRow::new(runtime, native_host));
    }
}

/// One framework's contribution to a snapshot. A release declaration the
/// release law rejects fails the contribution typed.
pub type CatalogRowProvider = fn(&mut CatalogRows) -> Result<(), ReleaseDescriptorError>;

/// Why a snapshot could not be composed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogSnapshotError {
    /// Two rows of one table share a key.
    Duplicate(DuplicateCatalogIdentity),
    /// A compiler backend whose carrier (adapter × carrier language ×
    /// framework epoch) has no frontend row: nothing could parse what it
    /// compiles.
    BackendWithoutFrontend(Box<CatalogIdentity>),
    /// A compiler backend whose runtime and native-host registrations name
    /// different carriers or epochs.
    BackendHalvesDisagree {
        /// The runtime registration's identity.
        runtime: Box<CatalogIdentity>,
        /// The native-host registration's identity.
        native_host: Box<CatalogIdentity>,
    },
    /// A provider declared a release the release law rejects.
    RejectedManifest(ReleaseDescriptorError),
}

impl From<DuplicateCatalogIdentity> for CatalogSnapshotError {
    fn from(duplicate: DuplicateCatalogIdentity) -> Self {
        Self::Duplicate(duplicate)
    }
}

/// Whether the catalog registers the capability a product needs, for one
/// carrier. Absence is a status, never an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductCapability {
    /// The product's capability is registered.
    Supported,
    /// The product's capability is not registered for this carrier.
    Unsupported(CatalogCapability),
}

/// The immutable catalog: one table per capability family, no row type
/// spanning tables. Built once from providers; no insert after.
#[derive(Debug)]
pub struct CatalogSnapshot {
    pub(super) carriers: CatalogTable<CarrierFrontendRow>,
    semantic: CatalogTable<SemanticAuthorityRow>,
    projection: CatalogTable<ProjectionBackendRow>,
    pub(super) compiler_backends: CatalogTable<CarrierCompilerBackendRow>,
}

/// Whether two identities name the same carrier under the same framework
/// profile.
fn same_carrier_epoch(a: &CatalogIdentity, b: &CatalogIdentity) -> bool {
    a.adapter_id() == b.adapter_id()
        && a.carrier() == b.carrier()
        && a.framework_profile() == b.framework_profile()
}

impl CatalogSnapshot {
    /// Compose a snapshot from providers, in order. Fails typed on a
    /// duplicate key in any table, on a compiler backend whose two halves
    /// name different carriers, and on a compiler backend whose carrier has
    /// no frontend row.
    pub fn compose(providers: &[CatalogRowProvider]) -> Result<Self, CatalogSnapshotError> {
        let mut rows = CatalogRows::default();
        for provide in providers {
            provide(&mut rows).map_err(CatalogSnapshotError::RejectedManifest)?;
        }
        let CatalogRows {
            carriers,
            semantic,
            projection,
            compiler_backends,
        } = rows;
        let snapshot = Self {
            carriers: CatalogTable::try_from_rows(carriers)?,
            semantic: CatalogTable::try_from_rows(semantic)?,
            projection: CatalogTable::try_from_rows(projection)?,
            compiler_backends: CatalogTable::try_from_rows(compiler_backends)?,
        };
        for backend in snapshot.compiler_backends.iter() {
            let runtime = backend.identity();
            let native_host = backend.host_identity();
            if !same_carrier_epoch(runtime, native_host) {
                return Err(CatalogSnapshotError::BackendHalvesDisagree {
                    runtime: Box::new(runtime.clone()),
                    native_host: Box::new(native_host.clone()),
                });
            }
            if !snapshot
                .carriers
                .iter()
                .any(|carrier| same_carrier_epoch(carrier.identity(), runtime))
            {
                return Err(CatalogSnapshotError::BackendWithoutFrontend(Box::new(
                    runtime.clone(),
                )));
            }
        }
        Ok(snapshot)
    }

    /// The carrier frontend registry over this snapshot.
    #[must_use]
    pub fn frontends(&self) -> CarrierFrontendRegistry<'_> {
        CarrierFrontendRegistry::over(self)
    }

    /// The carrier compiler-backend registry over this snapshot.
    #[must_use]
    pub fn compiler_backends(&self) -> CarrierCompilerBackendRegistry<'_> {
        CarrierCompilerBackendRegistry::over(self)
    }

    /// The semantic authority registered for one carrier × framework
    /// profile. Unknown or mismatched identity is `None`: no framework
    /// fallback.
    #[must_use]
    pub fn semantic_for(
        &self,
        carrier: &CarrierProfileId,
        profile: &FrameworkProfileId,
    ) -> Option<&InstalledSemanticAuthority> {
        record_registry_dispatch(RegistryTable::Semantic);
        self.semantic
            .iter()
            .find(|row| {
                let identity = row.identity();
                identity.carrier() == carrier && identity.framework_profile() == profile
            })
            .map(|row| row.semantic())
    }

    /// The projection backend registered for one carrier × framework
    /// profile. Unknown or mismatched identity is `None`: no framework
    /// fallback.
    #[must_use]
    pub fn projection_for(
        &self,
        carrier: &CarrierProfileId,
        profile: &FrameworkProfileId,
    ) -> Option<&InstalledProjectionBackend> {
        record_registry_dispatch(RegistryTable::Projection);
        self.projection
            .iter()
            .find(|row| {
                let identity = row.identity();
                identity.carrier() == carrier && identity.framework_profile() == profile
            })
            .map(|row| row.projection())
    }

    /// Every semantic-authority row, in catalog order. Iteration is not a
    /// dispatch.
    pub fn semantic_rows(&self) -> impl Iterator<Item = &SemanticAuthorityRow> {
        self.semantic.iter()
    }

    /// Every projection-backend row, in catalog order. Iteration is not a
    /// dispatch.
    pub fn projection_rows(&self) -> impl Iterator<Item = &ProjectionBackendRow> {
        self.projection.iter()
    }

    /// Whether the capability `kind` needs is registered for one carrier ×
    /// framework profile: a runtime product needs a compiler backend, an IDE,
    /// public-API or declaration product a projection backend, and an
    /// analysis product a semantic authority.
    #[must_use]
    pub fn product_capability(
        &self,
        carrier: &CarrierProfileId,
        profile: &FrameworkProfileId,
        kind: ProductKind,
    ) -> ProductCapability {
        let (present, capability) = match kind {
            ProductKind::RuntimeClient | ProductKind::RuntimeServer => (
                self.compiler_backends().row_for(carrier, profile).is_some(),
                CatalogCapability::Runtime,
            ),
            ProductKind::IdeCompanion | ProductKind::PublicApi | ProductKind::Declarations => (
                self.projection_for(carrier, profile).is_some(),
                CatalogCapability::Projection,
            ),
            ProductKind::Analysis => (
                self.semantic_for(carrier, profile).is_some(),
                CatalogCapability::Semantic,
            ),
        };
        if present {
            ProductCapability::Supported
        } else {
            ProductCapability::Unsupported(capability)
        }
    }
}
