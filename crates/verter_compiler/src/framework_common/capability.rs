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
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FrameworkEpochId(std::sync::Arc<str>);

impl FrameworkEpochId {
    /// Intern-free epoch id from a stable spelling.
    #[must_use]
    pub fn new(id: &str) -> Self {
        Self(std::sync::Arc::from(id))
    }

    /// Epoch spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Host integration epoch identity (catalog key component for host rows).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HostEpochId(std::sync::Arc<str>);

impl HostEpochId {
    /// Host-epoch spelling.
    #[must_use]
    pub fn new(id: &str) -> Self {
        Self(std::sync::Arc::from(id))
    }

    /// Epoch spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
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
pub trait FrameworkSemanticAuthority<FrameworkEpoch>: Send + Sync + 'static {
    /// Position-preserving eval source.
    type EvalSource: Send + Sync + 'static;
    /// Admitted template facts.
    type TemplateFacts: Send + Sync + 'static;
    /// Framework-owned style meaning facts.
    type StyleMeaning: Send + Sync + 'static;
    /// Admission token issued over an already-admitted parse.
    type SemanticAdmission: Send + Sync + 'static;
    /// Epoch marker consumed only as a type parameter.
    const _EPOCH: PhantomData<FrameworkEpoch> = PhantomData;
}

/// IDE companion, public-API, and declarations (TSC / `.d.ts`) projection.
pub trait ProjectionBackend: Send + Sync + 'static {
    /// IDE companion surface.
    type IdeCompanion: Send + Sync + 'static;
    /// Public API projection.
    type PublicApi: Send + Sync + 'static;
    /// Declaration / TSC splice shape.
    type Declarations: Send + Sync + 'static;
}

/// Runtime emit with statically selected targets; emits admitted facts only.
pub trait RuntimeCompilerBackend<FrameworkEpoch>: Send + Sync + 'static {
    /// Client runtime product.
    type RuntimeClient: Send + Sync + 'static;
    /// Server runtime product.
    type RuntimeServer: Send + Sync + 'static;
    /// Epoch marker consumed only as a type parameter.
    const _EPOCH: PhantomData<FrameworkEpoch> = PhantomData;
}

/// Host/unplugin/session integration; composes parse + semantic into compile admission.
pub trait FrameworkHostIntegrationBackend<FrameworkEpoch, HostEpoch>:
    Send + Sync + 'static
{
    /// The sole compile-admission token type for this host epoch.
    type CompileAdmission: Send + Sync + 'static;
    /// Epoch markers consumed only as type parameters.
    const _EPOCH: PhantomData<(FrameworkEpoch, HostEpoch)> = PhantomData;
}
