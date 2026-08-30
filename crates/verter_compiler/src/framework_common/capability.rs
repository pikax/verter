//! Typed compiler capability traits.
//!
//! Five authorities only. Associated types keep framework-private and
//! host-owned payloads off the generic catalog. Absence is type-level:
//! [`Present`] versus an omitted registration constructor, never a stub
//! backend or optional method that pretends a missing product exists.

use std::marker::PhantomData;

use verter_language::ParseOptions;

/// Marker wrapping a capability implementation that is actually present.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Present<T>(pub T);

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
    /// re-parse. Must not plan or publish a runtime product.
    fn project_ide(
        &self,
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
    /// publish an IDE companion.
    fn compile_runtime(
        &self,
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
