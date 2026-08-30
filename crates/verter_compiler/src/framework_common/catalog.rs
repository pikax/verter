//! Immutable compiler capability catalog.
//!
//! Uniqueness key: adapter × epoch × capability (host epoch is additional
//! uniqueness for host-integration rows). Language is row data, not a
//! uniqueness axis. Built once from typed registrations; no insert,
//! replace, or unload after construction. Capability is minted by the
//! matching `register_*` constructor and cannot disagree with the payload.

use std::collections::HashSet;

use verter_language::carrier_grammar::CarrierGrammarConfig;
use verter_language::{FrameworkAdapterId, LanguageId};

use super::capability::{
    CarrierFrontend, FrameworkEpoch, FrameworkEpochId, FrameworkHostIntegrationBackend,
    FrameworkSemanticAuthority, HostEpoch, HostEpochId, Present, ProjectionBackend,
    RuntimeCompilerBackend,
};

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
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CatalogIdentity {
    adapter_id: FrameworkAdapterId,
    carrier_language_id: LanguageId,
    epoch: FrameworkEpochId,
    host_epoch: Option<HostEpochId>,
    capability: CatalogCapability,
}

impl CatalogIdentity {
    fn ordinary(
        adapter_id: FrameworkAdapterId,
        carrier_language_id: LanguageId,
        epoch: FrameworkEpochId,
        capability: CatalogCapability,
    ) -> Self {
        debug_assert_ne!(capability, CatalogCapability::HostIntegration);
        Self {
            adapter_id,
            carrier_language_id,
            epoch,
            host_epoch: None,
            capability,
        }
    }

    fn host_integration(
        adapter_id: FrameworkAdapterId,
        carrier_language_id: LanguageId,
        epoch: FrameworkEpochId,
        host_epoch: HostEpochId,
    ) -> Self {
        Self {
            adapter_id,
            carrier_language_id,
            epoch,
            host_epoch: Some(host_epoch),
            capability: CatalogCapability::HostIntegration,
        }
    }

    /// Adapter identity.
    #[must_use]
    pub fn adapter_id(&self) -> &FrameworkAdapterId {
        &self.adapter_id
    }

    /// Carrier language identity.
    #[must_use]
    pub fn carrier_language_id(&self) -> &LanguageId {
        &self.carrier_language_id
    }

    /// Framework epoch value stored on the row.
    #[must_use]
    pub fn epoch(&self) -> &FrameworkEpochId {
        &self.epoch
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

    fn key(&self) -> CatalogKey<'_> {
        CatalogKey {
            adapter_id: self.adapter_id.as_str(),
            epoch: self.epoch.as_str(),
            capability: self.capability,
            host_epoch: self.host_epoch.as_ref().map(HostEpochId::as_str),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct CatalogKey<'a> {
    adapter_id: &'a str,
    epoch: &'a str,
    capability: CatalogCapability,
    host_epoch: Option<&'a str>,
}

/// Duplicate adapter × epoch × capability (× host epoch) identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateCatalogIdentity {
    /// The identity that collided.
    pub identity: CatalogIdentity,
}

/// Frozen catalog row: a closed capability arm plus its `Present` payload.
///
/// Uniqueness is still [`CatalogKey`] (adapter × epoch × capability, plus
/// host epoch for host-integration). The row never publishes identity
/// without the typed registration that minted it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogRow<F, P, S, R, H> {
    /// Frontend registration with its present frontend.
    Frontend(TypedCapabilityRegistration<FrontendCap<F>>),
    /// Projection registration with its present backend.
    Projection(TypedCapabilityRegistration<ProjectionCap<P>>),
    /// Semantic-authority registration with its present authority.
    Semantic(TypedCapabilityRegistration<SemanticCap<S>>),
    /// Runtime registration with its present backend.
    Runtime(TypedCapabilityRegistration<RuntimeCap<R>>),
    /// Host-integration registration with its present backend.
    HostIntegration(TypedCapabilityRegistration<HostCap<H>>),
}

impl<F, P, S, R, H> CatalogRow<F, P, S, R, H> {
    /// Identity of this typed row.
    #[must_use]
    pub fn identity(&self) -> &CatalogIdentity {
        match self {
            Self::Frontend(row) => row.identity(),
            Self::Projection(row) => row.identity(),
            Self::Semantic(row) => row.identity(),
            Self::Runtime(row) => row.identity(),
            Self::HostIntegration(row) => row.identity(),
        }
    }

    /// Present frontend when this row is a frontend registration.
    #[must_use]
    pub fn frontend(&self) -> Option<&F> {
        match self {
            Self::Frontend(row) => Some(row.frontend()),
            _ => None,
        }
    }

    /// Registered carrier-grammar fact when this row is a frontend
    /// registration carrying one; `None` for every other row.
    #[must_use]
    pub fn registered_grammar(&self) -> Option<&CarrierGrammarConfig> {
        match self {
            Self::Frontend(row) => row.registered_grammar(),
            _ => None,
        }
    }

    /// Present projection backend when this row is a projection registration.
    #[must_use]
    pub fn projection(&self) -> Option<&P> {
        match self {
            Self::Projection(row) => Some(row.projection()),
            _ => None,
        }
    }

    /// Present semantic authority when this row is a semantic registration.
    #[must_use]
    pub fn semantic(&self) -> Option<&S> {
        match self {
            Self::Semantic(row) => Some(row.semantic()),
            _ => None,
        }
    }

    /// Present runtime backend when this row is a runtime registration.
    #[must_use]
    pub fn runtime(&self) -> Option<&R> {
        match self {
            Self::Runtime(row) => Some(row.runtime()),
            _ => None,
        }
    }

    /// Present host-integration backend when this row is a host registration.
    #[must_use]
    pub fn host_integration(&self) -> Option<&H> {
        match self {
            Self::HostIntegration(row) => Some(row.host_integration()),
            _ => None,
        }
    }
}

impl<F, P, S, R, H> From<TypedCapabilityRegistration<FrontendCap<F>>>
    for CatalogRow<F, P, S, R, H>
{
    fn from(row: TypedCapabilityRegistration<FrontendCap<F>>) -> Self {
        Self::Frontend(row)
    }
}

impl<F, P, S, R, H> From<TypedCapabilityRegistration<ProjectionCap<P>>>
    for CatalogRow<F, P, S, R, H>
{
    fn from(row: TypedCapabilityRegistration<ProjectionCap<P>>) -> Self {
        Self::Projection(row)
    }
}

impl<F, P, S, R, H> From<TypedCapabilityRegistration<SemanticCap<S>>>
    for CatalogRow<F, P, S, R, H>
{
    fn from(row: TypedCapabilityRegistration<SemanticCap<S>>) -> Self {
        Self::Semantic(row)
    }
}

impl<F, P, S, R, H> From<TypedCapabilityRegistration<RuntimeCap<R>>> for CatalogRow<F, P, S, R, H> {
    fn from(row: TypedCapabilityRegistration<RuntimeCap<R>>) -> Self {
        Self::Runtime(row)
    }
}

impl<F, P, S, R, H> From<TypedCapabilityRegistration<HostCap<H>>> for CatalogRow<F, P, S, R, H> {
    fn from(row: TypedCapabilityRegistration<HostCap<H>>) -> Self {
        Self::HostIntegration(row)
    }
}

/// Process-lifetime immutable catalog of typed rows.
///
/// Construction accepts typed registrations only. Duplicate keys fail; there
/// is no later mutation and no identity-only catalog that can diverge.
#[derive(Debug, Clone)]
pub struct ImmutableCapabilityCatalog<F, P, S, R, H> {
    rows: Box<[CatalogRow<F, P, S, R, H>]>,
}

impl<F, P, S, R, H> ImmutableCapabilityCatalog<F, P, S, R, H> {
    /// Build a frozen catalog from typed rows. Duplicate keys fail.
    pub fn try_from_rows<I>(rows: I) -> Result<Self, DuplicateCatalogIdentity>
    where
        I: IntoIterator<Item = CatalogRow<F, P, S, R, H>>,
    {
        let mut rows: Vec<CatalogRow<F, P, S, R, H>> = rows.into_iter().collect();
        let mut seen = HashSet::with_capacity(rows.len());
        for row in &rows {
            if !seen.insert(row.identity().key()) {
                return Err(DuplicateCatalogIdentity {
                    identity: row.identity().clone(),
                });
            }
        }
        rows.sort_by(|a, b| a.identity().cmp(b.identity()));
        Ok(Self {
            rows: rows.into_boxed_slice(),
        })
    }

    /// Deterministic row iteration (sorted by adapter, language, epoch, host epoch, capability).
    pub fn iter(&self) -> impl Iterator<Item = &CatalogRow<F, P, S, R, H>> {
        self.rows.iter()
    }

    /// Number of frozen rows.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the catalog is empty.
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
    ) -> TypedCapabilityRegistration<FrontendCap<F>> {
        TypedCapabilityRegistration {
            identity: CatalogIdentity::ordinary(
                adapter_id,
                carrier_language_id,
                FrameworkEpochId::from_type::<E>(),
                CatalogCapability::Frontend,
            ),
            capability: FrontendCap {
                frontend,
                grammar: None,
            },
        }
    }

    /// Projection-only registration. No runtime backend is in the type.
    ///
    /// Catalog epoch identity is derived from `E`. The projection trait
    /// has no epoch type parameter; the constructor still takes `E`.
    pub fn register_projection<E: FrameworkEpoch, P: ProjectionBackend>(
        adapter_id: FrameworkAdapterId,
        carrier_language_id: LanguageId,
        projection: Present<P>,
    ) -> TypedCapabilityRegistration<ProjectionCap<P>> {
        TypedCapabilityRegistration {
            identity: CatalogIdentity::ordinary(
                adapter_id,
                carrier_language_id,
                FrameworkEpochId::from_type::<E>(),
                CatalogCapability::Projection,
            ),
            capability: ProjectionCap(projection),
        }
    }

    /// Semantic-authority registration.
    ///
    /// Catalog epoch identity is derived from `E`. There is no separate
    /// epoch-value argument that could disagree with the authority type.
    pub fn register_semantic<E: FrameworkEpoch, S: FrameworkSemanticAuthority<E>>(
        adapter_id: FrameworkAdapterId,
        carrier_language_id: LanguageId,
        semantic: Present<S>,
    ) -> TypedCapabilityRegistration<SemanticCap<S>> {
        TypedCapabilityRegistration {
            identity: CatalogIdentity::ordinary(
                adapter_id,
                carrier_language_id,
                FrameworkEpochId::from_type::<E>(),
                CatalogCapability::Semantic,
            ),
            capability: SemanticCap(semantic),
        }
    }

    /// Runtime-capable registration with a real backend (not a stub).
    ///
    /// Catalog epoch identity is derived from `E`.
    pub fn register_runtime<E: FrameworkEpoch, R: RuntimeCompilerBackend<E>>(
        adapter_id: FrameworkAdapterId,
        carrier_language_id: LanguageId,
        runtime: Present<R>,
    ) -> TypedCapabilityRegistration<RuntimeCap<R>> {
        TypedCapabilityRegistration {
            identity: CatalogIdentity::ordinary(
                adapter_id,
                carrier_language_id,
                FrameworkEpochId::from_type::<E>(),
                CatalogCapability::Runtime,
            ),
            capability: RuntimeCap(runtime),
        }
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
    ) -> TypedCapabilityRegistration<HostCap<H>> {
        TypedCapabilityRegistration {
            identity: CatalogIdentity::host_integration(
                adapter_id,
                carrier_language_id,
                FrameworkEpochId::from_type::<E>(),
                HostEpochId::from_type::<HostE>(),
            ),
            capability: HostCap(host),
        }
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

    /// Re-wrap the present runtime backend, keeping this row's identity.
    #[must_use]
    pub fn map_runtime<S>(
        self,
        map: impl FnOnce(R) -> S,
    ) -> TypedCapabilityRegistration<RuntimeCap<S>> {
        TypedCapabilityRegistration {
            identity: self.identity,
            capability: RuntimeCap(Present(map(self.capability.0 .0))),
        }
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
