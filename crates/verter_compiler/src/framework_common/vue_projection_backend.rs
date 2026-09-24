//! Vue [`ProjectionBackend`] adapter.
//!
//! Projects the IDE companion over an already-admitted parse through the
//! parsed compiler core and an IDE-only [`CompileRequest`]. Catalog lookup
//! keys adapter × epoch × Projection.

use verter_language::{
    syntax_profile_id_for, FileLanguage, FrameworkAdapterId, LanguageId, ParseOptions,
    SyntaxProfileId,
};

use crate::code_transform::CodeTransform;
use crate::compile::types::{
    CompileDiagnostic, VerterTsxBlock, VueExecutionInputs, VueMacroSemanticInput,
};
use crate::compile_request::{CompileRequest, ProductKind};
use crate::framework_common::capability::{Present, ProjectionBackend};
use crate::framework_common::carrier_compiler::{
    CompileUnsupported, IdeOutput, RuntimeBlockContentInputs, RuntimeOutputDescriptor,
    SourceMapFidelity,
};
use crate::framework_common::catalog::{ProjectionCap, TypedCapabilityRegistration};
use crate::framework_common::generated_chunk::{
    compose_generated_chunk, GeneratedFragment, GeneratedUnit,
};
use crate::framework_common::projection_plan::origin::{
    EmissionRefusal, ProjectionEmission, RoleQualifiedObservation,
};
use crate::framework_common::projection_plan::{
    build_projection_plan, incomplete_missing_parse, incomplete_parse_snapshot_mismatch, PlanInput,
    ProjectionPlan,
};
use crate::framework_common::vue_bridge::VueCarrierCompiler;
use crate::framework_common::vue_carrier_frontend::VueSfcV3;
use crate::framework_common::FrameworkParseArtifact;
use crate::ide::vue_projection::attribute_operations::project_attribute_operations;
use crate::ide::vue_projection::binder_capture::{capture_binder_plan, BinderCapturePlan};
use crate::ide::vue_projection::binding_views::{project_binding_views, BindingViewsProjection};
use crate::ide::vue_projection::options_api::project_options_pair;

/// STP13 named products: the acceptance surface of
/// [`VueProjectionBackend::options_projection`]. Re-exported here (rather
/// than through `ide`, which is crate-internal) so qualification harnesses
/// and the STP58 activation owner name the same types as this backend.
pub use crate::ide::vue_projection::options_api::{
    CombinedScriptProjection, OptionsComponentProjection, OptionsTemplateBindingView,
};

/// Attribute-operation products: the acceptance surface of
/// [`VueProjectionBackend::attribute_operations`]. Re-exported here (rather
/// than through `ide`, which is crate-internal) so qualification harnesses
/// and the use-inference and event consumers name the same types as this
/// backend.
pub use crate::ide::vue_projection::attribute_operations::{
    AttributeConsumerRelation, AttributeOperationsProjection, AttributeSyntax, Certainty,
    ConsumerChannel, Contribution, EffectiveProperty, MergeRule, OpaqueSpread, RuntimeKey,
    RuntimePropertyKeyPlan, RuntimePropertyWrite, SpreadKind, ValidationObligation, VueAttributeOp,
    VueAttributeSequence, WriteValue,
};

/// STP15 named products: the acceptance surface of
/// [`VueProjectionBackend::binding_views`]. Re-exported here (rather
/// than through `ide`, which is crate-internal) so qualification harnesses
/// and the STP58 activation owner name the same types as this backend.
pub use crate::ide::vue_projection::binding_views::{
    BindingKind, BindingUsageSet, TemplateReadBinding, TemplateReadView, TemplateWriteTarget,
    UsageRegions, UsedBinding, ViewSnapshotKind, WritableBinding, WriteDomain, WriteRejection,
};
use crate::ide::vue_projection::public_constructor::project_public_constructor;
/// Public constructor products: the acceptance surface of
/// [`VueProjectionBackend::public_constructor`] and the backend's
/// [`ProjectionBackend::PublicApi`]. Re-exported here (rather than through
/// `ide`, which is crate-internal) so qualification harnesses and the
/// declaration and use-inference consumers name the same types as this
/// backend.
pub use crate::ide::vue_projection::public_constructor::{
    ConstructorCompatibilityReceipt, ConstructorSource, DeclaredSurface, ExposeSurface,
    ExposedMember, InstanceMemberOrigin, PropsDefaults, PropsRequirement, PublicBinderParam,
    PublicInstanceMember, PublicInstanceProjection, PublicModel, PublicSurface, StaticOptions,
    VuePublicConstructorContract, EXPOSE_PROVIDER, PUBLIC_COMPONENT, PUBLIC_INSTANCE, PUBLIC_PROPS,
};
use crate::ide::vue_projection::script_setup::{
    project_script_pair, ScriptBlockInput, ScriptProjectionFacts, SetupProjectionRefusal,
};
use crate::parser::types::RootNodeScript;
use crate::standalone::{DirectCompileError, StandaloneCompiler};

/// The two authored script blocks of one carrier plus the `generic`
/// attribute value: normal script, setup script, generic.
type AuthoredScriptBlocks<'s> = (
    Option<ScriptBlockInput<'s>>,
    Option<ScriptBlockInput<'s>>,
    Option<&'s str>,
);

/// Vue IDE projection backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VueProjectionBackend;

/// Compile diagnostic tagged with the source space it was emitted against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VueProjectionDiagnostic {
    /// The compile diagnostic.
    pub diagnostic: CompileDiagnostic,
    /// Source-space token for this diagnostic's origin.
    pub source_space_token: String,
}

/// IDE companion plus compile diagnostics retained from the parsed core.
#[derive(Debug, Clone)]
pub struct VueIdeCompanion {
    /// Generated TSX/JSX companion.
    pub ide: IdeOutput,
    /// Non-fatal compile diagnostics from the parsed core, each tagged with
    /// the source space they were emitted against.
    pub diagnostics: Vec<VueProjectionDiagnostic>,
}

/// Execution inputs excluded from projection-request identity.
#[derive(Debug, Clone, Default)]
pub struct VueProjectionInputs {
    /// Host-selected block bytes for multi-unit IDE composition.
    pub block_content: RuntimeBlockContentInputs,
    /// Resolved Vue facts threaded beside the request.
    pub execution: VueExecutionInputs,
    /// Authoritative Vue macro semantics, when supplied.
    pub macros: VueMacroSemanticInput,
}

/// Typed Vue IDE projection refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VueProjectionError {
    /// Compile-unsupported refusal.
    Unsupported(CompileUnsupported),
    /// The request named a product other than the IDE companion.
    NotIdeOnly {
        /// Product the IDE projection will not emit.
        unexpected: ProductKind,
    },
    /// Parsed-core refusal that is not a [`CompileUnsupported`].
    Direct(DirectCompileError),
}

impl VueProjectionBackend {
    /// Adapter this backend answers to.
    #[must_use]
    pub fn adapter_id(&self) -> FrameworkAdapterId {
        VueCarrierCompiler.adapter_id()
    }

    /// Carrier language this backend projects.
    #[must_use]
    pub fn carrier_language_id(&self) -> LanguageId {
        VueCarrierCompiler.carrier_language_id()
    }

    /// Source-backed projection plan. Dormant relative to [`Self::project_ide`]:
    /// Vue atomic activation owns the live-route switch. Qualification
    /// harnesses call this path directly. CodeTransform may consume only
    /// [`ProjectionPlan::syntax_obligations`].
    #[must_use]
    pub fn projection_plan(
        &self,
        source: &str,
        artifact: &FrameworkParseArtifact,
        canonical_id: &str,
    ) -> ProjectionPlan {
        match VueCarrierCompiler.parsed_sfc(artifact) {
            Some(parsed) => {
                let exact_source = artifact.carrier_source().as_ref() == source;
                if !exact_source {
                    return incomplete_parse_snapshot_mismatch(canonical_id, source);
                }
                build_projection_plan(PlanInput {
                    canonical_id,
                    source,
                    parsed,
                    parse_key: Some(artifact.parse_key()),
                    syntax_profile: Some(artifact.syntax_profile()),
                })
            }
            None => incomplete_missing_parse(canonical_id, source),
        }
    }

    /// Statement-oriented script facts (module scope, setup statements,
    /// universal generic binder) from the admitted parse. Dormant relative to
    /// [`Self::project_ide`]; STP58 owns Vue atomic activation.
    pub fn script_projection(
        &self,
        source: &str,
        artifact: &FrameworkParseArtifact,
    ) -> Result<ScriptProjectionFacts, SetupProjectionRefusal> {
        let (normal, setup, generic) = self.script_blocks(source, artifact)?;
        project_script_pair(normal, setup, generic)
    }

    /// Classic Options API plus combined-script facts for the admitted
    /// parse: Options members, combined module/Options scope, and the
    /// template binding view inputs. Dormant relative to
    /// [`Self::project_ide`]; STP58 owns Vue atomic activation.
    pub fn options_projection(
        &self,
        source: &str,
        artifact: &FrameworkParseArtifact,
    ) -> Result<CombinedScriptProjection, SetupProjectionRefusal> {
        let (normal, setup, generic) = self.script_blocks(source, artifact)?;
        project_options_pair(normal, setup, generic)
    }

    /// Ordered attribute operations, runtime property keys and consumer
    /// relations for every component use of the admitted parse, derived
    /// from the same [`ProjectionPlan`] operations and expressions. An
    /// incomplete plan or an unjoinable use yields an incomplete product.
    /// Dormant relative to [`Self::project_ide`] until Vue atomic
    /// activation switches the live route.
    pub fn attribute_operations(
        &self,
        source: &str,
        artifact: &FrameworkParseArtifact,
        canonical_id: &str,
    ) -> Result<AttributeOperationsProjection, SetupProjectionRefusal> {
        let parsed = VueCarrierCompiler
            .parsed_sfc(artifact)
            .ok_or(SetupProjectionRefusal::MissingParse)?;
        if artifact.carrier_source().as_ref() != source {
            return Err(SetupProjectionRefusal::MissingParse);
        }
        let plan = self.projection_plan(source, artifact, canonical_id);
        Ok(project_attribute_operations(&plan, parsed, source))
    }

    /// Live template read/write views for the admitted parse: unwrapped
    /// top-level ref reads, setter-domain write targets with getter-only
    /// and readonly refusals, and no usage scaffolding (usage accounting
    /// stays with [`BindingUsageSet`], built from authored references
    /// only). Dormant relative to [`Self::project_ide`];
    /// STP58 owns Vue atomic activation.
    pub fn binding_views(
        &self,
        source: &str,
        artifact: &FrameworkParseArtifact,
    ) -> Result<BindingViewsProjection, SetupProjectionRefusal> {
        let (normal, setup, generic) = self.script_blocks(source, artifact)?;
        project_binding_views(normal, setup, generic)
    }

    /// Actual usage accounting over the admitted carrier's own regions:
    /// declared names come from [`project_binding_views`] read rows while
    /// template, script and style bytes are sliced from the admitted
    /// parse, never caller-supplied. Dormant relative to
    /// [`Self::project_ide`]; STP58 owns Vue atomic activation.
    pub fn binding_usage(
        &self,
        source: &str,
        artifact: &FrameworkParseArtifact,
    ) -> Result<BindingUsageSet, SetupProjectionRefusal> {
        let parsed = VueCarrierCompiler
            .parsed_sfc(artifact)
            .ok_or(SetupProjectionRefusal::MissingParse)?;
        let exact_source = artifact.carrier_source().as_ref() == source;
        if !exact_source {
            return Err(SetupProjectionRefusal::MissingParse);
        }
        let (normal, setup, generic) = self.script_blocks(source, artifact)?;
        let script = [
            normal.as_ref().map(|block| block.content),
            setup.as_ref().map(|block| block.content),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n");
        let views = project_binding_views(normal, setup, generic)?;
        let declared: Vec<String> = views
            .read
            .bindings
            .iter()
            .map(|binding| binding.name.clone())
            .collect();
        let template = parsed
            .template_ast()
            .and_then(|ast| ast.root.content.as_ref())
            .and_then(|content| source.get(content.start as usize..content.end as usize))
            .unwrap_or("");
        let mut style = String::new();
        for node in parsed.style_nodes() {
            if let Some(span) = node.content {
                if let Some(text) = source.get(span.start as usize..span.end as usize) {
                    style.push_str(text);
                    style.push('\n');
                }
            }
        }
        Ok(BindingUsageSet::from_region_text(
            &declared, template, &script, &style,
        ))
    }

    /// Binder-aware public dependency capture for the admitted parse: the
    /// free bindings of each public type surface, the script declarations
    /// reachable from them, and the binder parameters each lifted declaration
    /// must be re-bound over. Dormant relative to [`Self::project_ide`];
    /// STP58 owns Vue atomic activation.
    pub fn public_type_dependencies(
        &self,
        source: &str,
        artifact: &FrameworkParseArtifact,
    ) -> Result<BinderCapturePlan, SetupProjectionRefusal> {
        let (normal, setup, generic) = self.script_blocks(source, artifact)?;
        capture_binder_plan(normal, setup, generic)
    }

    /// The source-backed public constructor and instance contract for the
    /// admitted parse: one generic construct signature over the authored
    /// binder, the props/events/models/slots/exposed/static surface, the
    /// public instance, and its compatibility receipt. Dormant relative to
    /// [`Self::project_ide`], whose public carrier still answers until Vue
    /// atomic activation switches the live route.
    pub fn public_constructor(
        &self,
        source: &str,
        artifact: &FrameworkParseArtifact,
    ) -> Result<VuePublicConstructorContract, SetupProjectionRefusal> {
        let (normal, setup, generic) = self.script_blocks(source, artifact)?;
        project_public_constructor(normal, setup, generic)
    }

    /// The two authored script blocks and the `generic` attribute value of
    /// an admitted parse of exactly `source`. Both script-facing projections
    /// read the blocks here, so neither can drift onto a different notion of
    /// which bytes the authored blocks occupy.
    fn script_blocks<'s>(
        &self,
        source: &'s str,
        artifact: &FrameworkParseArtifact,
    ) -> Result<AuthoredScriptBlocks<'s>, SetupProjectionRefusal> {
        let parsed = VueCarrierCompiler
            .parsed_sfc(artifact)
            .ok_or(SetupProjectionRefusal::MissingParse)?;
        let exact_source = artifact.carrier_source().as_ref() == source;
        if !exact_source {
            return Err(SetupProjectionRefusal::MissingParse);
        }
        let block = |script: &RootNodeScript| {
            let span = script.content?;
            Some(ScriptBlockInput {
                content: source.get(span.start as usize..span.end as usize)?,
                content_start: span.start,
                lang: script.lang,
            })
        };
        let setup = parsed.script_setup();
        let generic = setup
            .and_then(|s| s.generic)
            .and_then(|span| source.get(span.start as usize..span.end as usize));
        Ok((
            parsed.script().and_then(block),
            setup.and_then(block),
            generic,
        ))
    }

    /// Checking text and correspondence from one CodeTransform record, bound
    /// to `plan`. Dormant relative to [`Self::project_ide`]: qualification
    /// harnesses call this path directly. STP58 owns Vue atomic activation.
    pub fn projection_emission(
        &self,
        transform: &CodeTransform<'_>,
        plan: &ProjectionPlan,
        canonical_id: &str,
        observations: Vec<RoleQualifiedObservation>,
    ) -> Result<ProjectionEmission, EmissionRefusal> {
        ProjectionEmission::from_plan(transform, plan, canonical_id, observations)
    }
}

impl ProjectionBackend for VueProjectionBackend {
    type IdeCompanion = VueIdeCompanion;
    type PublicApi = VuePublicConstructorContract;
    type Declarations = ();
    type ParseArtifact = FrameworkParseArtifact;
    type Request = CompileRequest;
    type ExecutionInputs = VueProjectionInputs;
    type Error = VueProjectionError;

    fn project_ide(
        &self,
        grant: crate::framework_common::capability::ProductExecutionGrant,
        source: &str,
        artifact: &FrameworkParseArtifact,
        request: &CompileRequest,
        inputs: &VueProjectionInputs,
    ) -> Result<VueIdeCompanion, VueProjectionError> {
        // Consume the demand's execution grant by value; a grant carved for
        // a different demand fails typed before any projection work runs.
        grant.consume_for(ProductKind::IdeCompanion).map_err(|_| {
            VueProjectionError::Unsupported(CompileUnsupported::ProductExecutionUngranted {
                product: ProductKind::IdeCompanion,
            })
        })?;
        require_ide_only(request)?;
        let Some(parsed) = VueCarrierCompiler.parsed_sfc(artifact) else {
            return Err(no_ide());
        };
        let requested_profile = requested_vue_syntax_profile(request)?;
        let exact_source = artifact.carrier_source().as_ref() == source;
        if !exact_source || artifact.syntax_profile() != &requested_profile {
            return Err(no_ide());
        }

        if inputs.block_content.script.is_some() || inputs.block_content.script_setup.is_some() {
            return Err(block_content_unavailable());
        }
        if inputs.block_content.template.is_some()
            && parsed.script().is_some()
            && parsed.script_setup().is_none()
        {
            return Err(block_content_unavailable());
        }

        let mut lowering = StandaloneCompiler
            .lower_vue_from_parsed(
                source,
                parsed,
                request,
                &inputs.execution,
                &inputs.macros,
                &inputs.block_content,
            )
            .map_err(map_direct)?;
        let tsx = lowering
            .result
            .tsx
            .take()
            .ok_or(VueProjectionError::Unsupported(
                CompileUnsupported::TargetMissingIde,
            ))?;
        match inputs.block_content.template.as_ref() {
            None => {
                let (space, artifact_token) = RuntimeOutputDescriptor::carrier_source(source);
                Ok(companion_from_tsx(
                    tsx,
                    wrap_projection_diagnostics(
                        lowering.result.errors,
                        lowering.selected_diagnostics,
                        &space,
                        None,
                    ),
                    &[(space.as_str(), artifact_token.as_str())],
                ))
            }
            Some(selected) => assemble_selected_template(
                source,
                tsx,
                lowering.result.errors,
                lowering.selected_diagnostics,
                selected,
            ),
        }
    }
}

fn requested_vue_syntax_profile(
    request: &CompileRequest,
) -> Result<SyntaxProfileId, VueProjectionError> {
    let vue = request.vue().ok_or_else(no_ide)?;
    let mut options = ParseOptions::vue_standard();

    if let Some(delimiters) = &vue.delimiters {
        options.delimiters = delimiters.clone();
    }
    options.custom_elements.clone_from(&vue.is_custom_element);

    syntax_profile_id_for(&FileLanguage::vue(), &options).map_err(|_| no_ide())
}

fn no_ide() -> VueProjectionError {
    VueProjectionError::Unsupported(CompileUnsupported::NoIdeProjection {
        adapter_id: FrameworkAdapterId::vue(),
    })
}

fn block_content_unavailable() -> VueProjectionError {
    VueProjectionError::Unsupported(CompileUnsupported::BlockContentIdeUnavailable {
        adapter_id: FrameworkAdapterId::vue(),
    })
}

fn require_ide_only(request: &CompileRequest) -> Result<(), VueProjectionError> {
    if request.vue().is_none() {
        return Err(no_ide());
    }
    let mut saw_ide = false;
    for product in request.products() {
        match product.kind() {
            ProductKind::IdeCompanion => saw_ide = true,
            unexpected => return Err(VueProjectionError::NotIdeOnly { unexpected }),
        }
    }
    if !saw_ide {
        return Err(VueProjectionError::Unsupported(
            CompileUnsupported::TargetMissingIde,
        ));
    }
    Ok(())
}

fn map_direct(err: DirectCompileError) -> VueProjectionError {
    match err {
        DirectCompileError::Vue(error) => {
            VueProjectionError::Unsupported(CompileUnsupported::RequestExecutionRefused(error))
        }
        DirectCompileError::UnsupportedProduct(ProductKind::IdeCompanion) => {
            VueProjectionError::Unsupported(CompileUnsupported::TargetMissingIde)
        }
        DirectCompileError::UnsupportedProduct(kind) => {
            VueProjectionError::NotIdeOnly { unexpected: kind }
        }
        other => VueProjectionError::Direct(other),
    }
}

fn ide_output_from_tsx(tsx: VerterTsxBlock, declared: &[(&str, &str)]) -> IdeOutput {
    let output_descriptor = RuntimeOutputDescriptor::generated(
        &tsx.code,
        (!tsx.source_map.is_empty()).then_some(tsx.source_map.as_str()),
        declared,
        SourceMapFidelity::Approximate,
    );
    IdeOutput {
        code: tsx.code,
        source_map: tsx.source_map,
        is_jsx: tsx.is_jsx,
        duration_ms: tsx.duration_ms,
        destructured_block: tsx.destructured_block,
        output_descriptor,
        generated_template_hole: tsx.generated_template_hole,
        generated_template_chunk: tsx.generated_template_chunk,
    }
}

fn wrap_projection_diagnostics(
    carrier_diagnostics: Vec<CompileDiagnostic>,
    selected_diagnostics: Vec<CompileDiagnostic>,
    carrier_token: &str,
    selected_token: Option<&str>,
) -> Vec<VueProjectionDiagnostic> {
    let mut diagnostics =
        Vec::with_capacity(carrier_diagnostics.len() + selected_diagnostics.len());
    diagnostics.extend(
        carrier_diagnostics
            .into_iter()
            .map(|diagnostic| VueProjectionDiagnostic {
                diagnostic,
                source_space_token: carrier_token.to_string(),
            }),
    );
    if let Some(selected_token) = selected_token {
        diagnostics.extend(selected_diagnostics.into_iter().map(|diagnostic| {
            VueProjectionDiagnostic {
                diagnostic,
                source_space_token: selected_token.to_string(),
            }
        }));
    }
    diagnostics
}

fn companion_from_tsx(
    tsx: VerterTsxBlock,
    diagnostics: Vec<VueProjectionDiagnostic>,
    declared: &[(&str, &str)],
) -> VueIdeCompanion {
    VueIdeCompanion {
        ide: ide_output_from_tsx(tsx, declared),
        diagnostics,
    }
}

fn assemble_selected_template(
    source: &str,
    mut shell: VerterTsxBlock,
    carrier_diagnostics: Vec<CompileDiagnostic>,
    selected_diagnostics: Vec<CompileDiagnostic>,
    selected: &crate::framework_common::RuntimeBlockContentInput,
) -> Result<VueIdeCompanion, VueProjectionError> {
    let hole = shell
        .generated_template_hole
        .clone()
        .ok_or_else(block_content_unavailable)?;
    let template_chunk = shell
        .generated_template_chunk
        .as_ref()
        .ok_or_else(block_content_unavailable)?;
    let (carrier_space, carrier_artifact) = RuntimeOutputDescriptor::carrier_source(source);
    let composed = compose_generated_chunk(
        "",
        GeneratedUnit {
            code: &shell.code,
            source_map: &shell.source_map,
            source_space: &carrier_space,
            source,
        },
        hole,
        GeneratedFragment {
            unit: GeneratedUnit {
                code: &template_chunk.code,
                source_map: &template_chunk.source_map,
                source_space: &selected.source_space_token,
                source: &selected.code,
            },
            range: 0..template_chunk.code.len() as u32,
        },
    )
    .ok_or_else(block_content_unavailable)?;
    shell.code = composed.code;
    shell.source_map = composed.source_map;
    shell.generated_template_hole = None;
    shell.generated_template_chunk = None;
    let declared = [
        (carrier_space.as_str(), carrier_artifact.as_str()),
        (
            selected.source_space_token.as_str(),
            selected.content_artifact_token.as_str(),
        ),
    ];
    Ok(companion_from_tsx(
        shell,
        wrap_projection_diagnostics(
            carrier_diagnostics,
            selected_diagnostics,
            &carrier_space,
            Some(&selected.source_space_token),
        ),
        &declared,
    ))
}

/// Typed Vue projection catalog row.
#[must_use]
pub fn vue_projection_backend_registration(
) -> TypedCapabilityRegistration<ProjectionCap<VueProjectionBackend>> {
    TypedCapabilityRegistration::register_projection::<VueSfcV3, _>(
        VueProjectionBackend.adapter_id(),
        VueProjectionBackend.carrier_language_id(),
        Present(VueProjectionBackend),
    )
}
