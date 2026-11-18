#![allow(dead_code, unused_variables)]

use oxc_allocator::Allocator;
use rustc_hash::FxHashMap;
use verter_audit::observe::ObserveExec;
use verter_css_syntax::StyleStage;

use super::artifact_set::{
    VueArtifactSet, VueArtifactSetError, VueComponentMetadata, VueCustomBlocks, VueModuleOutput,
    VueModuleSource, VueStyleMetadata,
};
use super::script_fragment::lower_script;
use crate::assembly::emit_plan::EmitPlanError;
use crate::assembly::{ArtifactSchemaError, VueMainDecoration};
use crate::ast::types::TemplateAst;
use crate::compile::style_dialect;
use crate::compile::types::{VueExecutionInputs, VueMacroSemanticInput};
use crate::compile_request::demand::{DemandRefusal, DemandSet, InputDemand};
use crate::compile_request::{CompileRequest, RuntimeStyleProcessing};
use crate::emission::QualifiedSource;
use crate::framework::vue::plan::VuePlanRefusal;
use crate::framework::vue::semantic::{
    build_semantic_snapshot_with_expressions, VueSemanticInputs, VueSemanticRefusal,
};
use crate::framework::vue::structure::{build_compile_structure, VueStructureRefusal};
use crate::framework::vue::style::{plan_vue_styles_in_scope, ScopeId, VueStyleBlockInput};
use crate::framework::vue::target::{VueTargetInputs, VueTargetOptions, VueTargetRefusal};
use crate::framework_common::{RuntimeBlockContentInputs, RuntimeDiagnostic};
use crate::parser::types::{ParsedSfc, RootNodeStyle};
use crate::style_planner::AuthoredStyleInput;
use crate::template::topology::{
    build_template_topology, TopologyBuildError, TopologyRecoveryFacts,
};
use crate::types::BindingType;
use crate::work_ledger::CompilerWorkLedger;

/// Everything one Vue runtime compile reads.
pub struct VueCompileInputs<'c> {
    pub source: &'c str,
    pub parsed: &'c ParsedSfc,
    /// A runtime-only request (client, server, or both).
    pub request: &'c CompileRequest,
    pub execution: &'c VueExecutionInputs,
    pub macros: &'c VueMacroSemanticInput,
    pub block_content: &'c RuntimeBlockContentInputs,
    pub decoration: &'c VueMainDecoration,
    pub canonical_id: &'c str,
}

#[derive(Debug)]
pub enum VueCompileRefusal {
    /// External inputs are required; resume with the same request.
    NeedInputs(Vec<InputDemand>),
    Admission(DemandRefusal),
    Plan(VuePlanRefusal),
    Topology(TopologyBuildError),
    Semantic(VueSemanticRefusal),
    Structure(VueStructureRefusal),
    Target(VueTargetRefusal),
    Emit(EmitPlanError),
    Artifact(ArtifactSchemaError),
    /// The source has errors; nothing publishes.
    InvalidSource {
        diagnostics: Vec<RuntimeDiagnostic>,
    },
}

/// Compile the runtime products of one component: close the demand, admit,
/// build the shared prerequisites once, run each requested target, assemble
/// the modules, and publish one artifact set.
pub fn compile_vue_runtime<M: ObserveExec>(
    inputs: &VueCompileInputs<'_>,
    allocator: &Allocator,
    ledger: &mut CompilerWorkLedger<M>,
) -> Result<VueArtifactSet, VueCompileRefusal> {
    use crate::assembly::emit_plan::MapMode;
    use crate::assembly::source_space::SourceSpaceKind;
    use crate::assembly::source_unit::{
        carrier_revision_of, carrier_source_id, ArtifactSourceUnit, ContentId, SourceUnit,
    };
    use crate::assembly::vue_module::{
        compose_main_fragments, VueMainFragmentsRequest, VueRenderPlans,
    };
    use crate::compile_request::demand::{
        close_demand, CompileAdmission, CompileProgress, ParseAdmission,
    };
    use crate::compile_request::{CompileProduct, ProductKind, ResolvedVueBackend};
    use crate::compile_transaction::{set_content_digest, CompileAttempt, DirectSetAdmission};
    use crate::framework::vue::plan::{plan_admitted, vue_demand_seeds};
    use crate::framework::vue::ssr::VueSsrExecutor;
    use crate::framework::vue::vapor::VueVaporExecutor;
    use crate::framework::vue::vdom::VueVdomExecutor;
    use crate::script::{
        bindings::script_lane_bindings, prepared::PreparedScript, ScriptCodeGenOptions,
    };
    use crate::target_plan::TargetExecutor;

    let request = inputs.request;
    let parsed = inputs.parsed;
    let source = inputs.source;
    let unsupported = |code| {
        VueCompileRefusal::Target(VueTargetRefusal::Unsupported {
            code,
            span: verter_span::Span::new(0, source.len() as u32),
        })
    };
    let products = request
        .products()
        .iter()
        .map(|product| match product {
            CompileProduct::RuntimeClient(runtime) | CompileProduct::RuntimeServer(runtime) => {
                Ok((product.kind(), runtime))
            }
            _ => Err(unsupported("runtime-module-target-unsupported")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let selected = inputs.block_content;
    if selected.script.is_some()
        || (selected.script_setup.is_some() && parsed.script().is_some())
        || selected.styles.iter().any(Option::is_some)
        || selected.custom_blocks.iter().any(Option::is_some)
    {
        return Err(unsupported("runtime-supplied-module-unsupported"));
    }
    let backend = request
        .resolve_vue_backend(parsed.is_vapor())
        .map_err(|error| VueCompileRefusal::Plan(VuePlanRefusal::Request(error)))?;
    let vue = request.vue().expect("the backend resolved a Vue request");
    let supplied_script = selected
        .script_setup
        .as_ref()
        .map(|input| crate::compile::parse_script_block(&input.code, &input.lang, true));
    let script_parsed = supplied_script.as_ref().unwrap_or(parsed);
    let script_source = selected
        .script_setup
        .as_ref()
        .map_or(source, |input| input.code.as_ref());
    let supplied_template = selected.template.as_ref().map(|input| {
        crate::compile::parse_template_block(
            &input.code,
            vue.delimiters
                .as_ref()
                .map(|(open, close)| (open.as_str(), close.as_str())),
            Some(&vue.is_custom_element),
        )
    });
    let template_parsed = supplied_template.as_ref().unwrap_or(parsed);
    let template_source = selected
        .template
        .as_ref()
        .map_or(source, |input| input.code.as_ref());
    let template = template_parsed.template_ast();
    let has_script = script_parsed.script().is_some() || script_parsed.script_setup().is_some();
    if backend == ResolvedVueBackend::Vapor
        && script_parsed.script_setup().is_some()
        && template.is_some()
        && products
            .iter()
            .any(|(_, runtime)| runtime.resolve_inline(request.is_production()))
    {
        return Err(unsupported("runtime-inline-module-unsupported"));
    }
    let inline_modes: Vec<_> = products
        .iter()
        .map(|(product, runtime)| {
            *product == ProductKind::RuntimeClient
                && backend == ResolvedVueBackend::Vdom
                && runtime.resolve_inline(request.is_production())
                && script_parsed.script_setup().is_some()
                && template.is_some()
        })
        .collect();
    let mut prepared = PreparedScript::build(
        script_source,
        script_parsed.script(),
        script_parsed.script_setup(),
        allocator,
    );
    let (diagnostics, runtime_valid) = crate::compile::runtime_source_diagnostics(
        script_parsed,
        template_parsed,
        &prepared,
        inputs.macros,
    );
    if parsed.has_errors()
        || template_parsed.has_errors()
        || !runtime_valid
        || diagnostics
            .iter()
            .any(|d| d.severity == crate::compile::CompileDiagnosticSeverity::Error)
    {
        return Err(VueCompileRefusal::InvalidSource {
            diagnostics: runtime_diagnostics(diagnostics, source),
        });
    }
    let macro_runtime = runtime_valid.then(|| inputs.macros.runtime()).flatten();
    let lane = script_lane_bindings(
        script_parsed.script_setup(),
        &prepared,
        macro_runtime,
        allocator,
    );
    let demand = close_demand(&vue_demand_seeds(
        request,
        backend,
        template.is_some(),
        has_script,
        parsed.style_nodes().len() as u32,
    ));
    let mode = if demand.contains(crate::compile_request::demand::Capability::MapRuntime) {
        MapMode::Map
    } else {
        MapMode::NoMap
    };
    let name = request
        .filename()
        .map(crate::compile::extract_component_name)
        .unwrap_or_else(|| "App".into());
    let content = ContentId::from_content_bytes(source.as_bytes());
    let unit = ArtifactSourceUnit {
        unit: SourceUnit::mint(
            carrier_source_id(inputs.canonical_id),
            carrier_revision_of(&content),
            "sfc",
            content.clone(),
        ),
        source_span: verter_span::Span::new(0, source.len() as u32),
    };
    let qualified = QualifiedSource {
        key: crate::emission::SourceKey {
            unit: unit.unit.id().clone(),
            content,
            space: SourceSpaceKind::Original,
        },
        text: source,
    };
    let supplied_script_source = selected.script_setup.as_ref().map(supplied_source);
    let supplied_template_source = selected.template.as_ref().map(supplied_source);
    let script_qualified = supplied_script_source
        .as_ref()
        .map_or(&qualified, |(_, source)| source);
    let render_qualified = supplied_template_source
        .as_ref()
        .map_or(&qualified, |(_, source)| source);
    let bindings = lane.bindings.clone();
    let args = VueTargetPrerequisiteArgs {
        retained: None,
        source: render_qualified.clone(),
        template,
        style_source: source,
        styles: parsed.style_nodes(),
        demand: &demand,
        bindings: &bindings,
        macros: inputs.macros,
        runtime_module: vue.runtime_module_name.as_deref().unwrap_or("vue"),
        component_id: request.component_id().unwrap_or(&name),
        scope: Some(ScopeId::for_runtime_component(
            &name,
            request.component_id(),
        )),
        prepared_styles: &inputs.execution.prepared_styles,
        style_source_name: request.filename().unwrap_or_default(),
        style_processing: request.runtime_style_processing(),
        options: VueTargetOptions {
            inline: false,
            is_production: request.is_production(),
            comments: vue.comments.unwrap_or(!request.is_production()),
            force_js: request.force_js(),
            self_name: crate::template::vue_syntax::to_pascal_case(&name),
            has_script,
            hoist_static: vue.hoist_static.unwrap_or(true),
            cache_handlers: vue.cache_handlers.unwrap_or(true),
            const_props: inputs.execution.prop_constness_overrides.clone(),
            ref_bindable_imports: lane
                .ref_bindable_imports
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
            map_mode: mode,
        },
    };
    let target_modes: Vec<_> = products
        .iter()
        .zip(&inline_modes)
        .map(|((_, runtime), &inline)| {
            let mode = if runtime.runtime_source_map {
                MapMode::Map
            } else {
                MapMode::NoMap
            };
            (mode, inline)
        })
        .collect();
    // The snapshot reads the lane; lowering later consumes that same lane.
    with_component_inputs(
        args,
        &target_modes,
        ledger,
        |targets, semantic, styles, blocks, style_diagnostics, ledger| {
            let styles = styles.expect("runtime modules build their style prerequisites");
            if !style_diagnostics.is_empty() {
                return Err(VueCompileRefusal::InvalidSource {
                    diagnostics: style_diagnostics,
                });
            }
            match CompileAdmission::compose(
                &demand,
                &ParseAdmission::complete(),
                semantic.admission(),
                styles.needed_inputs(),
            )
            .map_err(VueCompileRefusal::Admission)?
            {
                CompileProgress::NeedInputs(needed) => {
                    return Err(VueCompileRefusal::NeedInputs(needed))
                }
                CompileProgress::Admitted => {}
            }
            let plan = plan_admitted(request, Ok(backend), &demand, ledger)
                .map_err(VueCompileRefusal::Plan)?;
            let mut diagnostics = diagnostics;
            if template.is_some() {
                let mut expression_diagnostics = Vec::new();
                crate::compile::collect_expression_errors(
                    semantic.expressions(),
                    template_source,
                    &mut expression_diagnostics,
                    &mut diagnostics,
                );
                // Runtime emission cannot publish a recovered expression.
                if !expression_diagnostics.is_empty() {
                    diagnostics
                        .extend(crate::compile::convert_diagnostics(&expression_diagnostics));
                    return Err(VueCompileRefusal::InvalidSource {
                        diagnostics: runtime_diagnostics(diagnostics, source),
                    });
                }
            }
            let used = selected
                .template
                .is_none()
                .then(|| inputs.execution.template_used_vars.clone())
                .flatten()
                .or_else(|| {
                    template.map(|template| {
                        crate::compile::template_expr_overlay::collect_template_used_vars(
                            semantic.expressions(),
                            template,
                            template_source,
                        )
                        .0
                    })
                });
            let css_vars: Vec<_> = styles
                .scope
                .css_vars
                .iter()
                .map(|var| crate::style_planner::VBindVar {
                    expression: var.expression.clone(),
                    var_name: var.var_name.clone(),
                    expr_start: var.expression_span.start,
                    expr_end: var.expression_span.end,
                })
                .collect();
            prepared.prepare_css_var_expressions(&css_vars);
            let scope_id = styles.scope.scope.attribute();
            let dialect = crate::standalone::direct_vue_dialect(script_parsed, request.force_js());
            let mut modules = Vec::with_capacity(products.len());
            for (index, (product, runtime)) in products.into_iter().enumerate() {
                let target = targets.map(|targets| &targets[index]);
                let planned = plan.targets.iter().find(|target| target.product == product);
                // Template-less products still lower a script and publish a module.
                let mode = planned.map_or_else(
                    || {
                        if runtime.runtime_source_map {
                            MapMode::Map
                        } else {
                            MapMode::NoMap
                        }
                    },
                    |target| target.map_mode,
                );
                let output = target
                    .map(
                        |target| match planned.expect("a template has a planned target").target {
                            crate::framework::vue::plan::VueTarget::Ssr => {
                                VueSsrExecutor::execute(target, ledger)
                            }
                            crate::framework::vue::plan::VueTarget::Vdom => {
                                VueVdomExecutor::execute(target, ledger)
                            }
                            crate::framework::vue::plan::VueTarget::Vapor => {
                                VueVaporExecutor::execute(target, ledger)
                            }
                        },
                    )
                    .transpose()
                    .map_err(VueCompileRefusal::Target)?;
                let ssr = product == ProductKind::RuntimeServer;
                let inline = inline_modes[index];
                let script_options = ScriptCodeGenOptions {
                    macro_runtime,
                    is_production: request.is_production(),
                    custom_element: vue.script_custom_element.unwrap_or(false),
                    component_name: &name,
                    scope_id: &scope_id,
                    keep_ts_types: !request.force_js(),
                    inline_template: inline,
                    is_vapor: backend == ResolvedVueBackend::Vapor,
                    ssr,
                    has_scoped_style: styles.scope.has_scoped,
                    css_v_binds: &css_vars,
                    template_used_vars: used.clone(),
                };
                let script = lower_script::<M>(
                    script_qualified.clone(),
                    script_parsed,
                    &prepared,
                    crate::script::bindings::ScriptLaneBindings {
                        bindings: lane.bindings.clone(),
                        binding_order: lane.binding_order.clone(),
                        ref_bindable_imports: lane.ref_bindable_imports.clone(),
                    },
                    &script_options,
                    vue.runtime_module_name.as_deref().unwrap_or("vue"),
                    allocator,
                    mode,
                )
                .map_err(VueCompileRefusal::Emit)?;
                let script_helpers = script
                    .as_ref()
                    .map_or_else(Vec::new, |script| script.helpers());
                let script = script.map(|script| script.into_fragment(&script_helpers));
                if let Some(output) = &output {
                    if !output.diagnostics.is_empty() {
                        return Err(VueCompileRefusal::InvalidSource {
                            diagnostics: output
                                .diagnostics
                                .iter()
                                .map(|d| RuntimeDiagnostic {
                                    severity: d.severity,
                                    code: d.code.into(),
                                    message: d.message.clone(),
                                    span: d.span,
                                })
                                .collect(),
                        });
                    }
                }
                let helpers = output.as_ref().map(|output| output.helpers.clone());
                let render = output.map(|output| VueRenderPlans {
                    preamble: output.preamble,
                    render: output.render,
                    helpers: helpers.as_ref().expect("render helpers"),
                    render_export: output.render_export,
                });
                let module = compose_main_fragments(
                    VueMainFragmentsRequest {
                        canonical_id: inputs.canonical_id,
                        dialect,
                        planned_kind: product,
                        runtime: vue.runtime_module_name.as_deref().unwrap_or("vue"),
                        mode,
                        inline,
                        style_count: parsed.style_nodes().len(),
                        custom_block_count: parsed.unknown_nodes().len(),
                        scope_id: if styles.scope.has_scoped {
                            &scope_id
                        } else {
                            ""
                        },
                        script,
                        render,
                        decoration: inputs.decoration.clone(),
                    },
                    ledger,
                )
                .map_err(VueCompileRefusal::Emit)?;
                let sources = module
                    .sources()
                    .iter()
                    .map(|key| {
                        let candidates = std::iter::once((
                            &unit,
                            &qualified,
                            request.filename().unwrap_or_default(),
                        ))
                        .chain(supplied_script_source.as_ref().map(|(unit, source)| {
                            (
                                unit,
                                source,
                                selected
                                    .script_setup
                                    .as_ref()
                                    .unwrap()
                                    .source_space_token
                                    .as_str(),
                            )
                        }))
                        .chain(supplied_template_source.as_ref().map(|(unit, source)| {
                            (
                                unit,
                                source,
                                selected
                                    .template
                                    .as_ref()
                                    .unwrap()
                                    .source_space_token
                                    .as_str(),
                            )
                        }));
                        let (unit, source, name) = candidates
                            .into_iter()
                            .find(|(_, source, _)| source.key.same(key))
                            .ok_or(VueCompileRefusal::Artifact(
                                ArtifactSchemaError::MapSourceOutsideProvenance,
                            ))?;
                        Ok(VueModuleSource {
                            unit: unit.clone(),
                            name: if mode == MapMode::Map {
                                name.to_owned()
                            } else {
                                String::new()
                            },
                            text: (mode == MapMode::Map).then(|| std::sync::Arc::from(source.text)),
                        })
                    })
                    .collect::<Result<Vec<_>, VueCompileRefusal>>()?;
                modules.push(VueModuleOutput {
                    product,
                    dialect,
                    module,
                    sources,
                });
            }
            let attempt = CompileAttempt::enter_direct(source, request, "vue");
            let custom_blocks = crate::compile::custom_block_facts(parsed, source, || {
                qualified.key.content.clone()
            });
            let metadata = VueComponentMetadata {
                scope_id: styles.scope.scope.hash().to_owned(),
                has_scoped_style: styles.scope.has_scoped,
                inline_template: inline_modes.iter().any(|&inline| inline),
                has_script,
                has_template: template.is_some(),
                styles: blocks
                    .iter()
                    .map(|block| VueStyleMetadata {
                        index: block.index,
                        scoped: block.scoped,
                        module: block.module.map(str::to_owned),
                        lang: verter_css_syntax::CssDialect::LANG_SPELLINGS
                            .iter()
                            .find(|(_, dialect)| *dialect == block.authored.dialect)
                            .map_or("css", |(name, _)| *name)
                            .to_owned(),
                    })
                    .collect(),
                custom_blocks: custom_blocks
                    .iter()
                    .map(|block| block.block_type.clone())
                    .collect(),
            };
            let set = VueArtifactSet::build(
                DirectSetAdmission {
                    input_basis: attempt.input_basis().clone(),
                    canonical_id: inputs.canonical_id,
                    framework: "vue",
                    is_production: request.is_production(),
                    force_js: request.force_js(),
                    source_digest: set_content_digest(source.as_bytes()),
                    project: CompileAttempt::UNBOUND_PROJECT,
                },
                modules,
                &styles.blocks,
                blocks,
                &metadata,
                VueCustomBlocks {
                    source_len: source.len() as u32,
                    blocks: custom_blocks,
                },
                runtime_diagnostics(diagnostics, source),
            )
            .map_err(|error| artifact_refusal(error, source))?;
            let mut host_maps = Vec::new();
            if mode == MapMode::Map {
                for (input, supplied) in [
                    (
                        selected.script_setup.as_ref(),
                        supplied_script_source.as_ref(),
                    ),
                    (
                        selected.template.as_ref(),
                        supplied_template_source.as_ref(),
                    ),
                ] {
                    if let (Some(input), Some((unit, _))) = (input, supplied) {
                        if let Some(map) = input.source_map.as_ref().filter(|map| !map.is_empty()) {
                            host_maps.push((unit.unit.id().clone(), map.clone()));
                        }
                    }
                }
            }
            Ok(set.with_host_maps(host_maps))
        },
    )?
}

fn supplied_source(
    input: &crate::framework_common::RuntimeBlockContentInput,
) -> (crate::assembly::ArtifactSourceUnit, QualifiedSource<'_>) {
    use crate::assembly::source_unit::{
        carrier_revision_of, carrier_source_id, ArtifactSourceUnit, ContentId, SourceUnit,
    };
    let content = ContentId::from_content_bytes(input.code.as_bytes());
    let unit = ArtifactSourceUnit {
        unit: SourceUnit::mint(
            carrier_source_id(&input.source_space_token),
            carrier_revision_of(&content),
            &input.content_artifact_token,
            content.clone(),
        ),
        source_span: verter_span::Span::new(0, input.code.len() as u32),
    };
    let source = QualifiedSource {
        key: crate::emission::SourceKey {
            unit: unit.unit.id().clone(),
            content,
            space: crate::assembly::SourceSpaceKind::Original,
        },
        text: &input.code,
    };
    (unit, source)
}

fn runtime_diagnostics(
    diagnostics: Vec<crate::compile::CompileDiagnostic>,
    source: &str,
) -> Vec<RuntimeDiagnostic> {
    diagnostics
        .into_iter()
        .map(|d| RuntimeDiagnostic {
            severity: d.severity.into(),
            code: d.code,
            message: d.message,
            span: d
                .span
                .unwrap_or_else(|| verter_span::Span::new(0, source.len() as u32)),
        })
        .collect()
}

fn artifact_refusal(error: VueArtifactSetError, source: &str) -> VueCompileRefusal {
    let error = match error {
        VueArtifactSetError::InvalidModule { reason, .. } => {
            return VueCompileRefusal::InvalidSource {
                diagnostics: vec![RuntimeDiagnostic {
                    severity: crate::framework_common::RuntimeDiagnosticSeverity::Error,
                    code: "XInvalidExpression".into(),
                    message: reason,
                    span: verter_span::Span::new(0, source.len() as u32),
                }],
            }
        }
        VueArtifactSetError::Schema(error) => error,
        VueArtifactSetError::ModuleSourcesMismatch { .. } => {
            ArtifactSchemaError::MapSourceOutsideProvenance
        }
        VueArtifactSetError::ExternalMapUnsupported { .. } => ArtifactSchemaError::UnqualifiedMap,
        VueArtifactSetError::DuplicateModule { .. } => ArtifactSchemaError::DuplicateArtifact,
        VueArtifactSetError::NotARuntimeModule { .. } | VueArtifactSetError::Metadata(_) => {
            ArtifactSchemaError::InvalidIdentity
        }
        VueArtifactSetError::CustomBlock(error) => {
            return VueCompileRefusal::InvalidSource {
                diagnostics: vec![crate::framework_common::vue_bundle::custom_block_refusal(
                    error,
                    verter_span::Span::new(0, source.len() as u32),
                )],
            }
        }
        VueArtifactSetError::StyleRefused { .. }
        | VueArtifactSetError::MissingStyleBlock { .. } => ArtifactSchemaError::StyleFactsMismatch,
    };
    VueCompileRefusal::Artifact(error)
}

/// What the shared prerequisites of one target run are built from. Every
/// input is borrowed from a caller that has already parsed the component and
/// its script blocks; nothing here is parsed a second time.
pub(crate) struct VueTargetPrerequisiteArgs<'c> {
    /// The runtime overlay and completed style facts of a caller that
    /// already ran those owners.
    pub retained: Option<VueRetainedPrerequisites<'c>>,
    /// The SFC source with its identity; every template offset is absolute
    /// in it.
    pub source: QualifiedSource<'c>,
    pub template: Option<&'c TemplateAst>,
    /// Carrier bytes addressed by the style nodes, independently of the template.
    pub style_source: &'c str,
    pub styles: &'c [RootNodeStyle],
    /// The request's closed demand.
    pub demand: &'c DemandSet,
    /// The bindings the script lane reported.
    pub bindings: &'c FxHashMap<&'c str, BindingType>,
    pub macros: &'c VueMacroSemanticInput,
    /// Framework module specifier (`vue` by default).
    pub runtime_module: &'c str,
    /// Identity the component's scope attribute is hashed from.
    pub component_id: &'c str,
    /// Already resolved by the runtime request; absent on raw target runs.
    pub scope: Option<ScopeId>,
    pub prepared_styles: &'c [Option<crate::style_planner::PreparedStyleIr>],
    /// Name the style blocks report as their source.
    pub style_source_name: &'c str,
    pub style_processing: RuntimeStyleProcessing,
    pub options: VueTargetOptions,
}

#[derive(Clone, Copy)]
pub(crate) struct VueRetainedPrerequisites<'c> {
    pub expressions: &'c crate::template::oxc::types::OxcParsedAst<'c>,
    pub scope: &'c crate::framework::vue::style::VueComponentScopePlan,
}

/// Build the shared prerequisites of one target run, each once, and hand
/// them to `run`: the template topology, the semantic snapshot, the style
/// plan's component scope and the compile structure. The prerequisites
/// borrow each other, so they live only for the call.
pub(crate) fn with_target_inputs<M: ObserveExec, R>(
    args: VueTargetPrerequisiteArgs<'_>,
    ledger: &mut CompilerWorkLedger<M>,
    run: impl for<'a> FnOnce(&'a VueTargetInputs<'a>, &mut CompilerWorkLedger<M>) -> R,
) -> Result<R, VueCompileRefusal> {
    let target_mode = (args.options.map_mode, args.options.inline);
    with_component_inputs(
        args,
        &[target_mode],
        ledger,
        |inputs, _, _, _, _, ledger| {
            run(
                inputs
                    .and_then(|inputs| inputs.first())
                    .expect("a render prerequisite has a template"),
                ledger,
            )
        },
    )
}

fn with_component_inputs<M: ObserveExec, R>(
    args: VueTargetPrerequisiteArgs<'_>,
    target_modes: &[(crate::assembly::emit_plan::MapMode, bool)],
    ledger: &mut CompilerWorkLedger<M>,
    run: impl for<'a> FnOnce(
        Option<&'a [VueTargetInputs<'a>]>,
        &'a crate::framework::vue::semantic::VueSemanticSnapshot<'a>,
        Option<&'a crate::framework::vue::style::VueStylePlan>,
        &'a [VueStyleBlockInput<'a>],
        Vec<RuntimeDiagnostic>,
        &mut CompilerWorkLedger<M>,
    ) -> R,
) -> Result<R, VueCompileRefusal> {
    let VueTargetPrerequisiteArgs {
        retained,
        source,
        template,
        style_source,
        styles,
        demand,
        bindings,
        macros,
        runtime_module,
        component_id,
        scope,
        prepared_styles,
        style_source_name,
        style_processing,
        options,
    } = args;
    let text = source.text;
    let topology = template
        .map(|template| {
            build_template_topology(text, template, TopologyRecoveryFacts::default(), ledger)
        })
        .transpose()
        .map_err(VueCompileRefusal::Topology)?;
    let allocator = Allocator::default();
    let semantic = build_semantic_snapshot_with_expressions(
        VueSemanticInputs {
            source: text,
            topology: topology.as_ref(),
            reported_bindings: bindings.clone(),
            setup_analysis: None,
            companion_analysis: None,
            macros,
            runtime_module,
            self_name: &options.self_name,
            template_is_ts: false,
            allocator: &allocator,
        },
        demand,
        retained.map(|retained| retained.expressions),
        ledger,
    )
    .map_err(VueCompileRefusal::Semantic)?;
    let (blocks, style_diagnostics) = if retained.is_some() {
        (Vec::new(), Vec::new())
    } else {
        style_block_inputs(style_source, styles, style_source_name, prepared_styles)
    };
    let styles = retained.is_none().then(|| {
        plan_vue_styles_in_scope(
            scope.unwrap_or_else(|| ScopeId::for_component(component_id)),
            &blocks,
            Some(&semantic),
            style_processing == RuntimeStyleProcessing::Complete,
            options.map_mode,
            ledger,
        )
    });
    let structure = topology
        .as_ref()
        .map(|topology| {
            build_compile_structure(text, topology, &semantic, true, options.comments, ledger)
        })
        .transpose()
        .map_err(VueCompileRefusal::Structure)?;
    let target_options: Vec<_> = target_modes
        .iter()
        .map(|&(map_mode, inline)| VueTargetOptions {
            map_mode,
            inline,
            ..options.clone()
        })
        .collect();
    let inputs = topology
        .as_ref()
        .zip(structure.as_ref())
        .map(|(topology, structure)| {
            target_options
                .iter()
                .map(|options| VueTargetInputs {
                    source: source.clone(),
                    allocator: &allocator,
                    topology,
                    structure,
                    semantic: &semantic,
                    scope: retained.map_or_else(
                        || &styles.as_ref().expect("fresh style prerequisites").scope,
                        |retained| retained.scope,
                    ),
                    options,
                })
                .collect::<Vec<_>>()
        });
    Ok(run(
        inputs.as_deref(),
        &semantic,
        styles.as_ref(),
        &blocks,
        style_diagnostics,
        ledger,
    ))
}

/// The style blocks of a component as the style planner reads them: each
/// block's authored bytes in its declared dialect. A block without content
/// or in a dialect the rewrite cannot read contributes nothing to the scope.
fn style_block_inputs<'a>(
    source: &'a str,
    styles: &[RootNodeStyle],
    source_name: &'a str,
    prepared_styles: &'a [Option<crate::style_planner::PreparedStyleIr>],
) -> (Vec<VueStyleBlockInput<'a>>, Vec<RuntimeDiagnostic>) {
    let mut diagnostics = Vec::new();
    let blocks = styles
        .iter()
        .enumerate()
        .filter_map(|(index, style)| {
            let content = style.content?;
            let dialect = match style_dialect(style.lang) {
                Some(dialect) => dialect,
                None => {
                    diagnostics.extend(runtime_diagnostics(
                        crate::compile::convert_diagnostics(&[
                            crate::compile::unknown_style_diagnostic(content),
                        ]),
                        source,
                    ));
                    return None;
                }
            };
            let code = &source[content.start as usize..content.end as usize];
            Some(VueStyleBlockInput {
                index: u32::try_from(index).unwrap_or(u32::MAX),
                scoped: style.scoped,
                // The parser records only that `module` is present.
                module: style.module.then_some(""),
                authored: AuthoredStyleInput::new(
                    code,
                    dialect,
                    source_name,
                    "standalone:carrier",
                    "standalone:carrier-bytes",
                ),
                prepared: crate::style_planner::prepared_style_for_sealed_slot(
                    None,
                    prepared_styles,
                    index,
                    code,
                ),
                stage: StyleStage::Authored,
            })
        })
        .collect();
    (blocks, diagnostics)
}

#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod tests;
