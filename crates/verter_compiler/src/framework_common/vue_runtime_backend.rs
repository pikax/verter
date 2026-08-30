//! Vue [`RuntimeCompilerBackend`] adapter.
//!
//! Emits runtime products over an already-admitted parse through the
//! parsed compiler core and a runtime-only [`CompileRequest`]. Catalog
//! lookup keys adapter × epoch × Runtime.

use std::sync::Arc;

use rustc_hash::FxHashSet;
use verter_language::{
    syntax_profile_id_for, FileLanguage, FrameworkAdapterId, LanguageId, ParseOptions,
    SyntaxProfileId,
};
use verter_macro_dto::MacroRuntimeBundle;

use crate::assembly::AssembledArtifact;
use crate::compile::types::{TemplateBindingMetadata, VueExecutionInputs, VueMacroSemanticInput};
use crate::compile_request::{CompileRequest, CompileRequestError, ProductKind};
use crate::framework_common::capability::{Present, RuntimeCompilerBackend};
use crate::framework_common::carrier_compiler::RuntimeBlockContentInputs;
use crate::framework_common::catalog::{RuntimeCap, TypedCapabilityRegistration};
use crate::framework_common::vue_bridge::VueCarrierCompiler;
use crate::framework_common::vue_carrier_frontend::VueSfcV3;
use crate::framework_common::{CarrierCompiler, FrameworkParseArtifact};
use crate::standalone::{
    compile_vue_parsed_runtime, DirectCompileError, DirectCompileOutput, VueParsedRuntimeError,
};
use crate::style_planner::PreparedStyleIr;

/// Vue runtime compiler backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VueRuntimeBackend;

/// Macro-free Vue execution facts excluded from runtime-request identity.
#[derive(Debug, Clone, Default)]
pub struct VueRuntimeExecutionFacts {
    /// Cross-file const-prop overrides for template bindings.
    pub prop_constness_overrides: Option<FxHashSet<String>>,
    /// Binding names referenced in style `v-bind()` expressions.
    pub style_v_bind_vars: Vec<String>,
    /// Whether `style_v_bind_vars` is the complete usage inventory.
    pub style_v_bind_usage_complete: Option<bool>,
    /// Script semantics transferred into a separately compiled template.
    pub template_binding_metadata: Option<TemplateBindingMetadata>,
    /// Parser-derived identifiers from a separately parsed template unit.
    pub template_used_vars: Option<FxHashSet<String>>,
    /// Host-retained parsed style IRs in inventory order.
    pub prepared_styles: Vec<Option<PreparedStyleIr>>,
}

/// Execution inputs excluded from runtime-request identity.
#[derive(Debug, Clone, Default)]
pub struct VueRuntimeInputs {
    /// Host-selected block bytes for supplied templates, scripts, and styles.
    pub block_content: RuntimeBlockContentInputs,
    /// Resolved Vue facts threaded beside the request. Macro payloads are
    /// not representable here.
    pub execution: VueRuntimeExecutionFacts,
    /// Authoritative runtime macro projection, when supplied.
    pub macros: Option<Arc<MacroRuntimeBundle>>,
}

/// Typed Vue runtime refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VueRuntimeError {
    /// Selected block content cannot be compiled truthfully.
    BlockContentUnavailable,
    /// Canonical request execution refused after parse (SSR × vapor, …).
    RequestExecutionRefused(CompileRequestError),
    /// The request named a product other than a runtime target.
    NotRuntimeOnly {
        /// Product the runtime backend will not emit.
        unexpected: ProductKind,
    },
    /// The admitted artifact is not a usable Vue parse.
    UnusableParse,
    /// Requested source bytes do not match the admitted artifact.
    SourceMismatch,
    /// Requested syntax profile does not match the admitted artifact.
    ProfileMismatch,
    /// The request is not a Vue compile request.
    FrameworkMismatch,
    /// Parsed-core refusal that is not a runtime topology or request refusal.
    Direct(DirectCompileError),
}

impl VueRuntimeBackend {
    /// Adapter this backend answers to.
    #[must_use]
    pub fn adapter_id(&self) -> FrameworkAdapterId {
        VueCarrierCompiler.adapter_id()
    }

    /// Carrier language this backend compiles.
    #[must_use]
    pub fn carrier_language_id(&self) -> LanguageId {
        VueCarrierCompiler.carrier_language_id()
    }
}

impl RuntimeCompilerBackend<VueSfcV3> for VueRuntimeBackend {
    type RuntimeClient = AssembledArtifact;
    type RuntimeServer = AssembledArtifact;
    type ParseArtifact = FrameworkParseArtifact;
    type Request = CompileRequest;
    type ExecutionInputs = VueRuntimeInputs;
    type Error = VueRuntimeError;
    type Output = DirectCompileOutput;

    fn compile_runtime(
        &self,
        source: &str,
        artifact: &FrameworkParseArtifact,
        request: &CompileRequest,
        inputs: &VueRuntimeInputs,
    ) -> Result<DirectCompileOutput, VueRuntimeError> {
        require_runtime_only(request)?;
        let Some(parsed) = VueCarrierCompiler.parsed_sfc(artifact) else {
            return Err(VueRuntimeError::UnusableParse);
        };
        let requested_profile = requested_vue_syntax_profile(request)?;
        let exact_source = artifact
            .inventory()
            .source_spaces()
            .first()
            .is_some_and(|space| space.bytes().as_ref() == source);
        if !exact_source {
            return Err(VueRuntimeError::SourceMismatch);
        }
        if artifact.syntax_profile() != &requested_profile {
            return Err(VueRuntimeError::ProfileMismatch);
        }

        let macros = inputs
            .macros
            .clone()
            .map(VueMacroSemanticInput::Runtime)
            .unwrap_or_default();
        let execution = vue_execution_from_facts(&inputs.execution);

        compile_vue_parsed_runtime(
            source,
            parsed,
            request,
            &execution,
            &macros,
            &inputs.block_content,
            &inputs.execution.prepared_styles,
        )
        .map_err(map_parsed_runtime)
    }
}

fn vue_execution_from_facts(facts: &VueRuntimeExecutionFacts) -> VueExecutionInputs {
    VueExecutionInputs {
        prop_constness_overrides: facts.prop_constness_overrides.clone(),
        style_v_bind_vars: facts.style_v_bind_vars.clone(),
        style_v_bind_usage_complete: facts.style_v_bind_usage_complete,
        template_binding_metadata: facts.template_binding_metadata.clone(),
        template_used_vars: facts.template_used_vars.clone(),
        prepared_styles: facts.prepared_styles.clone(),
        ..VueExecutionInputs::default()
    }
}

fn requested_vue_syntax_profile(
    request: &CompileRequest,
) -> Result<SyntaxProfileId, VueRuntimeError> {
    let vue = request.vue().ok_or(VueRuntimeError::FrameworkMismatch)?;
    let mut options = ParseOptions::vue_standard();

    if let Some(delimiters) = &vue.delimiters {
        options.delimiters = delimiters.clone();
    }
    options.custom_elements.clone_from(&vue.is_custom_element);

    syntax_profile_id_for(&FileLanguage::vue(), &options)
        .map_err(|_| VueRuntimeError::ProfileMismatch)
}

fn require_runtime_only(request: &CompileRequest) -> Result<(), VueRuntimeError> {
    if request.vue().is_none() {
        return Err(VueRuntimeError::FrameworkMismatch);
    }
    let mut saw_runtime = false;
    for product in request.products() {
        match product.kind() {
            ProductKind::RuntimeClient | ProductKind::RuntimeServer => saw_runtime = true,
            unexpected => return Err(VueRuntimeError::NotRuntimeOnly { unexpected }),
        }
    }
    if !saw_runtime {
        return Err(VueRuntimeError::UnusableParse);
    }
    Ok(())
}

fn map_parsed_runtime(err: VueParsedRuntimeError) -> VueRuntimeError {
    match err {
        VueParsedRuntimeError::BlockContentUnavailable => VueRuntimeError::BlockContentUnavailable,
        VueParsedRuntimeError::RequestExecutionRefused(error) => {
            VueRuntimeError::RequestExecutionRefused(error)
        }
        VueParsedRuntimeError::Direct(DirectCompileError::UnsupportedProduct(
            kind @ (ProductKind::RuntimeClient | ProductKind::RuntimeServer),
        )) => VueRuntimeError::Direct(DirectCompileError::UnsupportedProduct(kind)),
        VueParsedRuntimeError::Direct(DirectCompileError::UnsupportedProduct(kind)) => {
            VueRuntimeError::NotRuntimeOnly { unexpected: kind }
        }
        VueParsedRuntimeError::Direct(DirectCompileError::Vue(error)) => {
            VueRuntimeError::RequestExecutionRefused(error)
        }
        VueParsedRuntimeError::Direct(other) => VueRuntimeError::Direct(other),
    }
}

/// Typed Vue runtime catalog row.
#[must_use]
pub fn vue_runtime_backend_registration(
) -> TypedCapabilityRegistration<RuntimeCap<VueRuntimeBackend>> {
    TypedCapabilityRegistration::register_runtime::<VueSfcV3, _>(
        VueRuntimeBackend.adapter_id(),
        VueRuntimeBackend.carrier_language_id(),
        Present(VueRuntimeBackend),
    )
}
