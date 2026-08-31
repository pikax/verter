//! Typed compiler capability traits.
//!
//! Five authorities only. Associated types keep framework-private and
//! host-owned payloads off the generic catalog. Absence is type-level:
//! [`Present`] versus an omitted registration constructor, never a stub
//! backend or optional method that pretends a missing product exists.

use std::marker::PhantomData;

use verter_language::ParseOptions;

use crate::compile_request::ProductKind;

/// Marker wrapping a capability implementation that is actually present.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Present<T>(pub T);

/// Consume-once execution evidence for ONE admitted compile-product
/// demand.
///
/// Product backends ([`ProjectionBackend::project_ide`],
/// [`RuntimeCompilerBackend::compile_runtime`]) and the shared bundle
/// orchestration legs require this value: driving a product backend
/// without it is unrepresentable, and because the grant is neither
/// `Clone` nor `Copy` and is consumed by value, one grant drives at most
/// one execution of its admitted demand.
///
/// The grant is demand-MULTIPLICITY evidence for one product-backend
/// leg; it is not artifact provenance — the admission's parse key is
/// what pairs issuance with execution. A grant is carved off a
/// host-issued compile admission by value (`into_execution_grants` on
/// the admission types) — the host-integration backend composes parse +
/// semantic into the admission and remains the sole issuer — or minted
/// crate-privately at the registry-dispatched
/// `compile_bundle`/`compile_ide` route boundaries, which carry no
/// host-issued admission. The inner field is private and the mint is
/// crate-private, so grant minting authority never leaves the crate and
/// an external caller cannot forge one.
#[derive(Debug)]
pub struct ProductExecutionGrant {
    admitted: ProductKind,
}

impl ProductExecutionGrant {
    /// Crate-internal mint. Reachable only from the admission carve and
    /// the registry bundle-route orchestration; grant-minting authority
    /// never leaves the crate.
    pub(crate) fn mint(admitted: ProductKind) -> Self {
        Self { admitted }
    }

    /// The product demand this grant admits.
    #[must_use]
    pub fn admits(&self, kind: ProductKind) -> bool {
        self.admitted == kind
    }

    /// Consume this grant for the named product leg: the demand-match
    /// check plus the by-value consumption in one step. A grant carved
    /// for a different demand fails typed with the kind it actually
    /// admitted — never a silent execution under the wrong demand.
    pub(crate) fn consume_for(self, product: ProductKind) -> Result<(), ProductKind> {
        if self.admitted == product {
            Ok(())
        } else {
            Err(self.admitted)
        }
    }
}

// Consume-once by-value evidence must never be duplicable or
// round-trippable through a serialized form.
static_assertions::assert_not_impl_any!(
    ProductExecutionGrant: Clone, Copy, serde::Serialize, serde::Deserialize<'static>
);
static_assertions::assert_not_impl_any!(
    ProductExecutionGrants: Clone, Copy, serde::Serialize, serde::Deserialize<'static>
);

/// The per-demand execution grants carved off ONE compile admission (or
/// minted crate-privately by the registry bundle route): at most one
/// grant per product-backend leg. Each slot is consume-once by value.
#[derive(Debug, Default)]
pub struct ProductExecutionGrants {
    /// Grant for the admitted runtime product leg, when one was admitted.
    pub runtime: Option<ProductExecutionGrant>,
    /// Grant for the admitted IDE-projection leg, when one was admitted.
    pub projection: Option<ProductExecutionGrant>,
}

/// Framework semantic epoch identity (catalog key component).
///
/// Derived from a [`FrameworkEpoch`] type. Callers do not invent this
/// independently of the register constructor's epoch type parameter.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FrameworkEpochId(std::sync::Arc<str>);

impl FrameworkEpochId {
    pub(crate) fn from_type<E: FrameworkEpoch>() -> Self {
        Self::new(E::ID)
    }

    /// Intern-free epoch id from a stable spelling.
    ///
    /// Catalog constructors derive identity from [`FrameworkEpoch::ID`]
    /// through [`Self::from_type`]; they do not take this value.
    #[must_use]
    pub(crate) fn new(id: &str) -> Self {
        Self(std::sync::Arc::from(id))
    }

    /// Epoch spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Typed framework epoch. The type is the authority; catalog identity is
/// derived from [`Self::ID`].
pub trait FrameworkEpoch: Send + Sync + 'static {
    /// Stable catalog spelling for this epoch type.
    const ID: &'static str;
}

/// Host integration epoch identity (catalog key component for host rows).
///
/// Derived from a [`HostEpoch`] type on host-integration registrations.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HostEpochId(std::sync::Arc<str>);

impl HostEpochId {
    pub(crate) fn from_type<H: HostEpoch>() -> Self {
        Self::new(H::ID)
    }

    /// Host-epoch spelling.
    #[must_use]
    pub(crate) fn new(id: &str) -> Self {
        Self(std::sync::Arc::from(id))
    }

    /// Epoch spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Typed host epoch. The type is the authority; catalog identity is
/// derived from [`Self::ID`].
pub trait HostEpoch: Send + Sync + 'static {
    /// Stable catalog spelling for this host epoch type.
    const ID: &'static str;
}

/// Typed native host epoch. Catalog identity is derived from
/// [`HostEpoch::ID`]. Host-neutral: every framework's host-integration
/// registration for the native host names this one epoch type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeHostEpoch;

impl HostEpoch for NativeHostEpoch {
    const ID: &'static str = "native-host";
}

/// Parse, registered geometry, unregistered parse artifacts, parse
/// diagnostics retained on the parse artifact, and syntax reject.
pub trait CarrierFrontend: Send + Sync + 'static {
    /// Registered or unregistered parse artifact for this frontend.
    type ParseArtifact: Send + Sync + 'static;
    /// Fail-closed syntax reject (no parse admission).
    type SyntaxReject: Send + Sync + 'static;
    /// Admission token issued only after a successful parse.
    type ParseAdmission: Send + Sync + 'static;

    /// Parse source through this frontend. Recoverable syntax stays `Ok`
    /// with mapped diagnostics; unsupported options reject before an
    /// artifact exists.
    fn parse(
        &self,
        source: &str,
        opts: &ParseOptions,
    ) -> Result<Self::ParseArtifact, Self::SyntaxReject>;
}

/// Per-framework interpretation: eval-source, template facts, style meaning.
pub trait FrameworkSemanticAuthority<E: FrameworkEpoch>: Send + Sync + 'static {
    /// Position-preserving eval source.
    type EvalSource: Send + Sync + 'static;
    /// Admitted template facts.
    type TemplateFacts: Send + Sync + 'static;
    /// Framework-owned style meaning facts.
    type StyleMeaning: Send + Sync + 'static;
    /// Admission token issued over an already-admitted parse.
    type SemanticAdmission: Send + Sync + 'static;
    /// Already-admitted parse artifact this authority consumes.
    type ParseArtifact: Send + Sync + 'static;
    /// Epoch marker consumed only as a type parameter.
    const _EPOCH: PhantomData<E> = PhantomData;

    /// Position-preserving eval source over an admitted parse artifact.
    fn eval_source(&self, source: &str, artifact: &Self::ParseArtifact) -> Self::EvalSource;

    /// Template facts over an admitted parse artifact. Does not re-parse.
    fn template_facts(&self, source: &str, artifact: &Self::ParseArtifact) -> Self::TemplateFacts;
}

/// IDE companion, public-API, and declarations (TSC / `.d.ts`) projection.
pub trait ProjectionBackend: Send + Sync + 'static {
    /// IDE companion surface.
    type IdeCompanion: Send + Sync + 'static;
    /// Public API projection.
    type PublicApi: Send + Sync + 'static;
    /// Declaration / TSC splice shape.
    type Declarations: Send + Sync + 'static;
    /// Already-admitted parse artifact this backend consumes.
    type ParseArtifact: Send + Sync + 'static;
    /// Typed projection request identity.
    type Request: Send + Sync + 'static;
    /// Execution inputs excluded from request identity (selected block bytes).
    type ExecutionInputs: Send + Sync + 'static;
    /// Typed projection refusal.
    type Error: Send + Sync + 'static;

    /// Project the IDE companion over an already-admitted parse. Does not
    /// re-parse. Must not plan or publish a runtime product. Consumes the
    /// demand's execution grant by value: one grant drives one projection,
    /// and a grant carved for a different demand fails typed.
    fn project_ide(
        &self,
        grant: ProductExecutionGrant,
        source: &str,
        artifact: &Self::ParseArtifact,
        request: &Self::Request,
        inputs: &Self::ExecutionInputs,
    ) -> Result<Self::IdeCompanion, Self::Error>;
}

/// Runtime emit with statically selected targets; emits admitted facts only.
pub trait RuntimeCompilerBackend<E: FrameworkEpoch>: Send + Sync + 'static {
    /// Client runtime product.
    type RuntimeClient: Send + Sync + 'static;
    /// Server runtime product.
    type RuntimeServer: Send + Sync + 'static;
    /// Already-admitted parse artifact this backend consumes.
    type ParseArtifact: Send + Sync + 'static;
    /// Typed runtime request identity.
    type Request: Send + Sync + 'static;
    /// Execution inputs excluded from request identity.
    type ExecutionInputs: Send + Sync + 'static;
    /// Typed runtime refusal.
    type Error: Send + Sync + 'static;
    /// Atomic runtime publication for every requested runtime target.
    type Output: Send + Sync + 'static;
    /// Epoch marker consumed only as a type parameter.
    const _EPOCH: PhantomData<E> = PhantomData;

    /// Compile requested runtime products over an already-admitted parse.
    /// Does not re-parse. One request shares parse, semantic, plan, and emit
    /// prerequisites across selected runtime targets. Must not plan or
    /// publish an IDE companion. Consumes the demand's execution grant by
    /// value: one grant drives one runtime compile, and a grant carved for
    /// a different demand fails typed.
    fn compile_runtime(
        &self,
        grant: ProductExecutionGrant,
        source: &str,
        artifact: &Self::ParseArtifact,
        request: &Self::Request,
        inputs: &Self::ExecutionInputs,
    ) -> Result<Self::Output, Self::Error>;
}

/// Host/unplugin/session integration; composes parse + semantic into compile admission.
///
/// Demand-specific issuance with ONE token type: the host-backed
/// multi-product demand and the runtime-render demand each yield a
/// [`Self::CompileAdmission`], and demand specificity is carried in the
/// issued admission's VALUE (the admitted demand plus the requested
/// product set) — never by sibling token types. Capability validation is
/// demand-specific on the implementing backend: a runtime-render demand
/// must not require projection capability, and a missing required
/// capability is a typed [`Self::AdmissionRefusal`] — never a fallback to
/// another lane, framework, or compatibility compiler.
///
/// The issued admission is consume-once: the backend's execution entries
/// take it by value, so one issuance drives at most one execution of the
/// admitted demand, and the per-demand [`ProductExecutionGrant`]s carved
/// off it are each consumed by the one product-backend leg they admit.
pub trait FrameworkHostIntegrationBackend<E: FrameworkEpoch, HostE: HostEpoch>:
    Send + Sync + 'static
{
    /// The sole compile-admission token type for this host epoch.
    type CompileAdmission: Send + Sync + 'static;
    /// Already-admitted parse artifact this backend composes over.
    type ParseArtifact: Send + Sync + 'static;
    /// Host-backed multi-product demand document (framework-owned shape:
    /// the requested product set plus the framework's typed options).
    type MultiProductDemand: Send + Sync + 'static;
    /// Runtime-render demand document (render-only; framework-owned shape).
    type RuntimeRenderDemand: Send + Sync + 'static;
    /// Typed issuance refusal: capability unavailability, request
    /// construction refusal, or a non-composable parse.
    type AdmissionRefusal: Send + Sync + 'static;
    /// Epoch markers consumed only as type parameters.
    const _EPOCH: PhantomData<(E, HostE)> = PhantomData;

    /// Issue one admission for a host-backed multi-product demand,
    /// composing this backend's parse + semantic admissions over the
    /// already-admitted artifact. The issued value records the admitted
    /// demand and the requested product set.
    fn admit_host_products(
        &self,
        artifact: &Self::ParseArtifact,
        demand: Self::MultiProductDemand,
    ) -> Result<Self::CompileAdmission, Self::AdmissionRefusal>;

    /// Issue one admission for a runtime-render (render-only) demand over
    /// the already-admitted artifact. Validation is demand-specific:
    /// projection capability is not required.
    fn admit_runtime_render(
        &self,
        artifact: &Self::ParseArtifact,
        demand: Self::RuntimeRenderDemand,
    ) -> Result<Self::CompileAdmission, Self::AdmissionRefusal>;
}
