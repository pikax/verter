//! # `compile_transaction` — the staged compile transaction (C2)
//!
//! [`CompileAttempt`] is the single transaction every compile route
//! enters: the direct facade stages its publication through
//! [`CompileAttempt::admit_published_products`], and the semantic
//! projections that feed a compile ride the same transaction through
//! [`CompileAttempt::type_info`] — the sealed [`CompileTypeInfo`]
//! gateway over `verter_semantic`'s non-flow kernel.
//!
//! The transaction owns three invariants the facade previously held
//! inline, plus the new request-local continuation contract
//! (`C2-AC-C1-A6-CONTINUATION-001`):
//!
//! * **One input basis.** The [`verter_identity::identity::InputBasisId`]
//!   of a conversion is minted once, inside the transaction, from the
//!   axes and digests bound at entry — callers never mint their own.
//! * **No stale publication.** Admitting a publication whose source no
//!   longer hashes to the entry-bound digest is a typed refusal, never a
//!   quietly rebased conversion.
//! * **Request-local sealed continuation.** Each type-info operation
//!   execution carries a private continuation identity binding the
//!   operation/request, the originating and current observation states,
//!   the resolution basis, the canonical frontier order, and every
//!   consumed observation version. Any changed, appeared, disappeared,
//!   reordered, or reconfigured input forces a whole-operation restart;
//!   invalidation, cancellation, terminal failure, and no-progress
//!   discard sealed unpublished output. Nothing survives the request:
//!   no cross-request cache, no public continuation DTO, no reusable
//!   warm state, no new retention authority.

pub mod type_info;

pub use type_info::{CompileTypeInfo, TypeInfoRouteFailure};

use std::sync::Arc;

use verter_identity::identity::InputBasisId;
use verter_macro_dto::RuntimePropType;
use verter_semantic::analysis::ScriptAnalysisSnapshot;
use verter_semantic::resolver_core::ResolutionBasis;
use verter_semantic::type_info::{ImportedComponentResolution, ObservedMacroSurface};

use crate::assembly::publish::ArtifactSchemaError;
use crate::compile_request::CompileRequest;

/// Domain identity of the direct facade's artifact-set conversion: one
/// stable logical source identity per `(carrier id, framework)` compile.
pub(crate) struct DirectSetTag<'a> {
    pub(crate) canonical_id: &'a str,
    pub(crate) framework: &'static str,
}

impl verter_identity::encoding::CanonicalEncode for DirectSetTag<'_> {
    const DOMAIN_TAG: &'static str = "verter.compiler.direct.compile.v1";
    fn encode_fields(&self, e: &mut verter_identity::encoding::CanonicalEncoder) {
        e.field_str(1, self.canonical_id);
        e.field_str(2, self.framework);
    }
}

/// The observed-input basis of one transaction: identity and request
/// axes plus DIGESTS of the authored source and of every published
/// product's bytes/maps — never the bytes themselves, so a set's identity
/// stays bounded by the size of its compile axes, not its output.
struct DirectSetInputBasis<'a> {
    canonical_id: &'a str,
    framework: &'static str,
    is_production: bool,
    force_js: bool,
    source_digest: [u8; 32],
    products_digest: [u8; 32],
}

impl verter_identity::encoding::CanonicalEncode for DirectSetInputBasis<'_> {
    const DOMAIN_TAG: &'static str = "verter.compiler.direct.compile.input_basis.v1";
    fn encode_fields(&self, e: &mut verter_identity::encoding::CanonicalEncoder) {
        e.field_str(1, self.canonical_id);
        e.field_str(2, self.framework);
        e.field_bool(3, self.is_production);
        e.field_bool(4, self.force_js);
        e.field_bytes(5, &self.source_digest);
        e.field_bytes(6, &self.products_digest);
    }
}

pub(crate) fn set_content_digest(bytes: &[u8]) -> [u8; 32] {
    *crate::assembly::source_unit::ContentId::from_content_bytes(bytes)
        .digest()
        .as_bytes()
}

/// How a transaction can refuse to admit a publication. The schema arm
/// wraps the assembly's own typed refusals; the source arm is the
/// transaction's no-stale-publication rail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileTransactionRefusal {
    /// The publication failed the canonical artifact schema.
    ArtifactSchema(ArtifactSchemaError),
    /// The source presented at admission no longer hashes to the
    /// digest bound when the transaction was entered.
    SourceChangedBetweenEnterAndAdmit,
}

impl CompileTransactionRefusal {
    /// The assembly-schema refusal, when this is one.
    pub const fn artifact_schema(&self) -> Option<&ArtifactSchemaError> {
        match self {
            Self::ArtifactSchema(error) => Some(error),
            Self::SourceChangedBetweenEnterAndAdmit => None,
        }
    }
}

impl From<ArtifactSchemaError> for CompileTransactionRefusal {
    fn from(error: ArtifactSchemaError) -> Self {
        Self::ArtifactSchema(error)
    }
}

/// The staged compile transaction. Entered once per compile; owns the
/// input-basis identity, the staged type-info observations, and the
/// request-local continuation state of the last driven operation.
pub struct CompileAttempt<'a> {
    canonical_id: &'a str,
    framework: &'static str,
    is_production: bool,
    force_js: bool,
    source_digest: [u8; 32],
    type_info: CompileTypeInfo,
}

static REQUEST_NONCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl<'a> CompileAttempt<'a> {
    /// Enter one direct compile transaction: binds the carrier identity,
    /// the request axes, and the authored source digest every later
    /// admission in this transaction must still agree with.
    pub fn enter_direct(
        source: &str,
        request: &'a CompileRequest,
        framework: &'static str,
    ) -> Self {
        let request_nonce = REQUEST_NONCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut type_info = CompileTypeInfo::new(ResolutionBasis::unbound_placeholder());
        type_info.bind_request_nonce(request_nonce);
        Self {
            canonical_id: request.filename().unwrap_or(""),
            framework,
            is_production: request.is_production(),
            force_js: request.force_js(),
            source_digest: set_content_digest(source.as_bytes()),
            type_info,
        }
    }

    /// Enter a semantic-only transaction: the session's TypeInfo producer
    /// enters here to drive the six projections, without a compile
    /// publication. Same transaction, same continuation contract, no
    /// admission facts.
    pub fn enter_semantic(owner_canonical: &str) -> Self {
        let request_nonce = REQUEST_NONCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut type_info = CompileTypeInfo::new(ResolutionBasis::unbound_placeholder());
        type_info.bind_request_nonce(request_nonce);
        Self {
            canonical_id: "",
            framework: "semantic",
            is_production: false,
            force_js: false,
            source_digest: set_content_digest(owner_canonical.as_bytes()),
            type_info,
        }
    }

    /// The sealed type-info gateway of this transaction: exactly the six
    /// projection methods, driving the semantic kernel over the staged
    /// observations.
    pub fn type_info(&mut self) -> &mut CompileTypeInfo {
        &mut self.type_info
    }

    /// Stage the immutable script analysis observation for one owner.
    /// Staging between operation rounds is what the continuation
    /// revalidates; a key restaged under a new version forces a
    /// whole-operation restart rather than a resumed stale answer.
    pub fn stage_script_analysis(
        &mut self,
        owner_canonical: Arc<str>,
        analysis: Arc<ScriptAnalysisSnapshot>,
    ) {
        self.type_info
            .stage_script_analysis(owner_canonical, analysis);
    }

    /// Stage one resolved import specifier observation.
    pub fn stage_import_resolution(
        &mut self,
        owner_canonical: Arc<str>,
        resolution: ImportedComponentResolution,
    ) {
        self.type_info
            .stage_import_resolution(owner_canonical, resolution);
    }

    /// Stage one observed macro surface.
    pub fn stage_macro_surface(
        &mut self,
        owner_canonical: Arc<str>,
        macro_index: usize,
        surface: ObservedMacroSurface,
    ) {
        self.type_info
            .stage_macro_surface(owner_canonical, macro_index, surface);
    }

    /// Stage one observed model value classification.
    pub fn stage_model_value_type_shape(
        &mut self,
        owner_canonical: Arc<str>,
        macro_index: usize,
        value_type_shape: RuntimePropType,
    ) {
        self.type_info
            .stage_model_value_type_shape(owner_canonical, macro_index, value_type_shape);
    }

    /// Cancel the in-flight operation: the continuation and its sealed
    /// unpublished output are discarded immediately.
    pub fn cancel(&mut self) {
        self.type_info.cancel();
    }

    /// Whether `source` still hashes to the entry-bound digest — the
    /// no-stale-publication rail admissions re-check.
    pub fn source_is_unchanged(&self, source: &str) -> bool {
        set_content_digest(source.as_bytes()) == self.source_digest
    }

    /// Admit one atomic publication into the transaction's staged
    /// contracts: mints the input basis from the entry-bound axes and
    /// digests, admits every published product to the canonical
    /// [`crate::assembly::publish::CompileArtifactSet`], and types the
    /// root of the request's primary runtime module. The publication
    /// stays the atomicity authority — this conversion only admits its
    /// already-published facts to the schema, moving each artifact's
    /// bytes into the set exactly once.
    ///
    /// # Errors
    ///
    /// [`CompileTransactionRefusal::SourceChangedBetweenEnterAndAdmit`]
    /// when `source` no longer hashes to the entry-bound digest;
    /// [`CompileTransactionRefusal::ArtifactSchema`] on any schema
    /// refusal during admission.
    pub(crate) fn admit_published_products(
        &self,
        source: &str,
        published: &crate::assembly::publish::ArtifactSet,
        qualified_styles: Vec<crate::framework_common::carrier_compiler::QualifiedRuntimeStyle>,
        custom_block_artifacts: Option<crate::assembly::publish::CompileArtifactSet>,
        diagnostics: Vec<crate::compile::CompileDiagnostic>,
    ) -> Result<crate::standalone::DirectCompileOutput, CompileTransactionRefusal> {
        if set_content_digest(source.as_bytes()) != self.source_digest {
            return Err(CompileTransactionRefusal::SourceChangedBetweenEnterAndAdmit);
        }
        let output = crate::standalone::admit_published_products(
            DirectSetAdmission {
                canonical_id: self.canonical_id,
                framework: self.framework,
                is_production: self.is_production,
                force_js: self.force_js,
                source_digest: self.source_digest,
            },
            source,
            published,
            qualified_styles,
            custom_block_artifacts,
            diagnostics,
        )
        .map_err(CompileTransactionRefusal::ArtifactSchema)?;
        Ok(output)
    }
}

/// The entry-bound facts one admission mints its input basis from.
pub(crate) struct DirectSetAdmission<'a> {
    pub(crate) canonical_id: &'a str,
    pub(crate) framework: &'static str,
    pub(crate) is_production: bool,
    pub(crate) force_js: bool,
    pub(crate) source_digest: [u8; 32],
}

/// The two identities one admission mints from the same encoded basis:
/// the input-basis id and the source-unit revision. Both derive from the
/// SAME [`DirectSetInputBasis`] encoding so a revision never disagrees
/// with the basis its artifacts carry.
pub(crate) struct MintedAdmissionIdentity {
    pub(crate) input_basis: InputBasisId,
    pub(crate) revision: crate::assembly::source_unit::SourceRevision,
}

pub(crate) fn mint_admission_identity(
    admission: &DirectSetAdmission<'_>,
    products_digest: [u8; 32],
) -> MintedAdmissionIdentity {
    let basis = DirectSetInputBasis {
        canonical_id: admission.canonical_id,
        framework: admission.framework,
        is_production: admission.is_production,
        force_js: admission.force_js,
        source_digest: admission.source_digest,
        products_digest,
    };
    MintedAdmissionIdentity {
        input_basis: InputBasisId::from_canonical(&basis),
        revision: crate::assembly::source_unit::SourceRevision::from_canonical(&basis),
    }
}

pub(crate) fn mint_direct_source_id(
    canonical_id: &str,
    framework: &'static str,
) -> crate::assembly::source_unit::SourceId {
    crate::assembly::source_unit::SourceId::from_canonical(&DirectSetTag {
        canonical_id,
        framework,
    })
}
