//! The checking module of one carrier: the single TypeScript module the
//! certified engine checks and that importers of the carrier resolve to.

use oxc_allocator::Allocator;
use rustc_hash::{FxHashMap, FxHashSet};
use verter_audit::observe::Uncaptured;
use verter_language::ScriptRegionKind;
use verter_parser::types::BindingType;
use verter_span::{GeneratedByteLen, SourceByteOffset, SourceByteRange, Span};

use crate::ast::types::TemplateAst;
use crate::code_transform::EditRecorder;
use crate::code_transform::{
    AnchorKind, CodeTransform, MappingProduct, MappingSpan, ProjectedClass, SegmentAnchor,
};
use crate::framework_common::projection_plan::origin::ProjectionEmission;
use crate::framework_common::projection_plan::{
    AdmittedExpressionId, ExpressionKind, ExpressionOccurrence, PlanSnapshotId, ProjectionPlan,
};
use crate::framework_common::vue_projection_backend::VueProjectionBackend;
use crate::framework_common::FrameworkParseArtifact;
use crate::ide::projection_declarations::DeclarationProjectionProduct;
use crate::ide::projection_recovery::{
    recover_script_regions, IncompleteProjectionState, ProjectionRegion, RecoveryBlock, RegionState,
};
use crate::ide::template::emit::{emit_op, trim_span, EmitOp, EmitText};
use crate::ide::vue_projection::attribute_operations::{
    project_attribute_operations, AttributeOperationsProjection,
};
use crate::ide::vue_projection::binder_capture::{
    BinderCapturePlan, DependencyOrigin, LiftedDeclarationKind,
};
use crate::ide::vue_projection::branches::{
    project_template_branches, TemplateBlock, TemplateBlockKind, TemplateBranchProjection,
    TemplateItem,
};
use crate::ide::vue_projection::builtins::{project_builtins, BuiltinProjection, CarrierShape};
use crate::ide::vue_projection::callback_reentry::{project_callback_reentry, CallbackReentryPlan};
use crate::ide::vue_projection::checking_fragment::{
    CheckingFragment, CheckingPiece, ObligationFamily, PlacedStatement, PreludeDeclaration,
    StatementAnchor, StatementPhase, UseHost,
};
use crate::ide::vue_projection::component_resolution::{
    project_component_resolution, ComponentResolutionProjection,
};
use crate::ide::vue_projection::component_use::{
    project_component_uses, ComponentUseProjection, WitnessComponent, USE_PRELUDE,
};
use crate::ide::vue_projection::composed_product::ComposedProduct;
use crate::ide::vue_projection::declarations::project_vue_declarations;
use crate::ide::vue_projection::directives::{project_directives, DirectiveSyntax};
use crate::ide::vue_projection::event_keys::project_event_transport;
use crate::ide::vue_projection::event_payloads::project_event_payloads;
use crate::ide::vue_projection::external_sources::{
    ExternalBlockKind, ExternalBlockProjectionBinding, ExternalLegality,
};
use crate::ide::vue_projection::fallthrough::project_fallthrough;
use crate::ide::vue_projection::generic_interop::ForeignComponentContractAdapter;
use crate::ide::vue_projection::iteration::project_iteration;
use crate::ide::vue_projection::js_dialect::{
    javascript_declarations, javascript_statements, NoJavaScriptForm,
};
use crate::ide::vue_projection::models::project_models;
use crate::ide::vue_projection::native_elements::project_native_elements;
use crate::ide::vue_projection::native_models::project_native_models_with_reentry;
use crate::ide::vue_projection::provided_slots::project_provided_slots;
use crate::ide::vue_projection::public_constructor::{
    render_binder_list, BinderSite, PublicDeclarationExtras, VuePublicConstructorContract,
};
use crate::ide::vue_projection::public_instance::project_public_instance;
use crate::ide::vue_projection::script_setup::{
    FilePragma, ScriptProjectionFacts, SetupProjectionRefusal, SetupStatementKind, SourceRange,
    TsSetupProjection,
};
use crate::ide::vue_projection::slot_outlets::project_slot_outlets;
use crate::ide::vue_projection::template_context::{
    project_template_context, TEMPLATE_CONTEXT, TEMPLATE_CONTEXT_PRELUDE,
};
use crate::parser::types::{
    sfc_script_dialect, sfc_script_lang_conflict, ParsedSfc, SfcScriptDialect,
};
use crate::script::prepared::PreparedScript;
use crate::template::topology::{
    build_template_topology, RegionCompleteness, TopologyRecoveryFacts,
};
use crate::template::vue_syntax::{escape_js_string_into, trim_handler_body};
use crate::work_ledger::CompilerWorkLedger;

/// Function that holds the setup statements, the template scope and the
/// `defineExpose` return. One per module; the setup body is checked once.
pub const SETUP_SCOPE: &str = "__VerterSetupScope";
/// Identity wrapper the `defineExpose` callee is replaced with.
pub const EXPOSED_VALUE: &str = "__VerterExposed";
/// Setup-scope binding that holds the exposed value the scope returns.
const EXPOSED_BINDING: &str = "__VerterExposedValue";
/// Declaration of [`EXPOSED_VALUE`]: the identity the exposed value passes
/// through, so the scope's return type is the authored argument's type.
const EXPOSED_VALUE_PRELUDE: PreludeDeclaration = PreludeDeclaration {
    key: "05-exposed-value",
    text: "declare function __VerterExposed<T>(exposed: T): T;\n",
};

/// Script dialect of the checking module; decides the virtual extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CheckingDialect {
    Ts,
    Tsx,
    Js,
    Jsx,
}

impl CheckingDialect {
    /// `"ts"`, `"tsx"`, `"js"` or `"jsx"`.
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Ts => "ts",
            Self::Tsx => "tsx",
            Self::Js => "js",
            Self::Jsx => "jsx",
        }
    }

    /// True for a JavaScript carrier's module, which is checked as
    /// JavaScript and carries its types in JSDoc.
    #[must_use]
    pub fn is_javascript(self) -> bool {
        matches!(self, Self::Js | Self::Jsx)
    }
}

/// The module's dialect is the carrier's authored script dialect, as the
/// parser classifies it: an absent `lang` is Vue's JavaScript default.
fn dialect_of(parsed: &ParsedSfc) -> CheckingDialect {
    match sfc_script_dialect(parsed.script_setup(), parsed.script()) {
        SfcScriptDialect::TypeScript => CheckingDialect::Ts,
        SfcScriptDialect::Tsx => CheckingDialect::Tsx,
        SfcScriptDialect::JavaScript => CheckingDialect::Js,
        SfcScriptDialect::Jsx => CheckingDialect::Jsx,
    }
}

/// Inputs of one composition.
#[derive(Debug, Clone, Copy)]
pub struct CheckingModuleInput<'a> {
    pub source: &'a str,
    pub artifact: &'a FrameworkParseArtifact,
    pub canonical_id: &'a str,
    /// Custom-element prefixes selected for this artifact.
    pub custom_elements: Option<&'a [String]>,
    /// External block binding; `None` when the carrier has no `src` block.
    pub external: Option<&'a ExternalBlockProjectionBinding>,
    /// Also build the carrier's declaration product. It adds a measurable
    /// share to each composition, so only a run that publishes declarations
    /// asks for it.
    pub declarations: bool,
}

/// A product that did not contribute because it is not implemented for this
/// carrier's inputs or refused them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithheldProduct {
    pub family: ObligationFamily,
    pub reason: WithheldReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WithheldReason {
    /// The product's entry point returned its pending value.
    Pending,
    /// The product was incomplete for this snapshot.
    Incomplete,
}

/// Generated span to obligation, in generated order. Offsets are bytes into
/// the checking text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObligationSpan {
    pub generated: SourceRange,
    pub family: ObligationFamily,
    pub host: Option<UseHost>,
}

/// The composed module.
#[derive(Debug, Clone)]
pub struct CheckingModule {
    pub dialect: CheckingDialect,
    pub snapshot: PlanSnapshotId,
    /// Checking text, correspondence and role-qualified observations.
    pub emission: ProjectionEmission,
    /// Statement spans by obligation family.
    pub obligations: Vec<ObligationSpan>,
    /// Recovery state; complete when the carrier parsed cleanly.
    pub recovery: IncompleteProjectionState,
    /// Products that contributed nothing.
    pub withheld: Vec<WithheldProduct>,
    /// The carrier's declaration product, built from the same public
    /// contract, binder capture and template declaration extras as the
    /// checking text. `None` unless the input asked for it, and when the
    /// composition holds no public contract to publish: a recovered script,
    /// or a carrier whose instance is its external script's module.
    pub declarations: Option<DeclarationProjectionProduct>,
}

impl CheckingModule {
    /// True when no product was withheld and recovery is complete. Only a
    /// complete module may warm a complete-result cache.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.withheld.is_empty() && self.recovery.is_complete()
    }
    #[must_use]
    pub fn checking_text(&self) -> &str {
        self.emission.checking_text()
    }
    /// Obligation that owns generated byte `offset`: the innermost span
    /// containing it.
    #[must_use]
    pub fn obligation_at(&self, offset: u32) -> Option<&ObligationSpan> {
        self.obligation_for_span(MappingSpan {
            start: offset,
            end: offset,
        })
    }

    /// The innermost recorded statement containing the complete checking
    /// span. A point belongs to the statement on its right; a range crossing
    /// statement boundaries has no single owner.
    #[must_use]
    pub fn obligation_for_span(&self, generated: MappingSpan) -> Option<&ObligationSpan> {
        if generated.end < generated.start {
            return None;
        }
        let after = self
            .obligations
            .partition_point(|span| span.generated.start <= generated.start);
        self.obligations[..after]
            .iter()
            .rev()
            .filter(|span| {
                generated.start < span.generated.end && generated.end <= span.generated.end
            })
            .min_by_key(|span| span.generated.end - span.generated.start)
    }
}

/// Why no module can be composed for a carrier. Each is reported against the
/// carrier as one diagnostic; none aborts a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckingModuleRefusal {
    /// The script blocks refused projection.
    Script(SetupProjectionRefusal),
    /// An external block is not legal Vue or not supported.
    External(crate::ide::vue_projection::external_sources::ExternalLegality),
    /// A fragment named an expression twice as `Expression`, or named an
    /// expression the plan does not hold. Always a defect in a product.
    FragmentInvariant {
        family: ObligationFamily,
        detail: String,
    },
    /// The composed edits could not be applied to the carrier. Always a
    /// defect in the layout or a product.
    Transform(crate::code_transform::CodeTransformError),
    /// The transform could not produce an emission.
    Emission(crate::framework_common::projection_plan::origin::EmissionRefusal),
}

/// Compose the checking module of one admitted carrier. One parse, one plan,
/// one topology and one pass over each product.
pub fn compose_checking_module(
    input: &CheckingModuleInput<'_>,
) -> Result<CheckingModule, CheckingModuleRefusal> {
    let CheckingModuleInput {
        source,
        artifact,
        canonical_id,
        custom_elements,
        external,
        declarations,
    } = *input;
    if let Some(refused) = external.and_then(first_refused_block) {
        return Err(CheckingModuleRefusal::External(refused));
    }
    let backend = VueProjectionBackend;
    let script = match backend.script_projection(source, artifact) {
        Ok(script) => script,
        Err(
            refusal @ (SetupProjectionRefusal::SyntaxErrors { .. }
            | SetupProjectionRefusal::InvalidGeneric),
        ) => return compose_recovered_script(input, refusal),
        Err(refusal) => return Err(CheckingModuleRefusal::Script(refusal)),
    };
    let public = backend
        .public_constructor(source, artifact)
        .map_err(CheckingModuleRefusal::Script)?;
    let capture = backend
        .public_type_dependencies(source, artifact)
        .map_err(CheckingModuleRefusal::Script)?;
    let plan = backend.projection_plan_with_custom_elements(
        source,
        artifact,
        canonical_id,
        custom_elements,
    );
    let parsed = backend
        .admitted_parse(source, artifact)
        .map_err(CheckingModuleRefusal::Script)?;
    // Vue refuses a pair whose decoded `lang` values differ, even when both
    // name one language (`js` beside `javascript`); the parser owns the rule.
    if sfc_script_lang_conflict(parsed.script_setup(), parsed.script()).is_some() {
        return Err(CheckingModuleRefusal::Script(
            SetupProjectionRefusal::ScriptLangConflict,
        ));
    }
    let attributes = project_attribute_operations(&plan, parsed, source);
    let uses = project_component_uses(&plan, &attributes);
    let resolution =
        project_component_resolution(&plan, &uses, &script, canonical_id, custom_elements);
    let shape = CarrierShape::of(parsed);
    let builtins = project_builtins(&plan, &resolution, shape);
    let fallbacks = builtins.render_fallbacks(&resolution);
    let fallbacks = if fallbacks.is_empty() {
        fallbacks
    } else {
        format!(
            "{}\n{}{fallbacks}",
            crate::ide::script::VUE_GLOBAL_COMPONENTS_AUGMENTATION,
            crate::ide::script::GLOBAL_COMPONENT_CHECKING_HELPERS,
        )
    };

    let regions = artifact.script_regions();
    let content = |kind: ScriptRegionKind| {
        regions
            .iter()
            .find(|region| region.kind == kind)
            .map(|region| SourceRange {
                start: region.span.start,
                end: region.span.end,
            })
    };
    let blocks = ScriptBlocks {
        normal: content(ScriptRegionKind::Module),
        setup: content(ScriptRegionKind::Instance),
    };
    let dialect = dialect_of(parsed);

    let template = match parsed.template_ast() {
        Some(ast) => {
            let products = TemplateProducts {
                source,
                artifact,
                custom_elements,
                plan: &plan,
                attributes: &attributes,
                uses: &uses,
                script: &script,
                public: &public,
                resolution: &resolution,
                builtins: &builtins,
                javascript: dialect.is_javascript(),
            };
            Some(compose_template_scope(&products, parsed, ast)?)
        }
        None => None,
    };
    let empty = LoweredScope::default();
    let lowered = template.as_ref().map_or(&empty, |scope| &scope.lowered);
    let declaration_extras = template
        .as_ref()
        .map(|scope| scope.declaration_extras.clone())
        .unwrap_or_default();
    let forwards_external_default = blocks.setup.is_none()
        && external.is_some_and(|binding| {
            binding
                .blocks
                .iter()
                .any(|block| block.kind == ExternalBlockKind::Script)
        });
    // An external script's instance is not a carrier-local constructor. Keep
    // its existing forwarding path until the external owner supplies a scope.
    let (mut ops, scope) = if forwards_external_default {
        (
            blocks
                .gaps(source.len() as u32)
                .into_iter()
                .map(delete)
                .collect(),
            None,
        )
    } else {
        let (ops, scope) = module_layout(
            source,
            &blocks,
            shape,
            &script,
            &public,
            &capture,
            &fallbacks,
            lowered,
            &declaration_extras,
            dialect,
        )?;
        (ops, Some(scope))
    };
    // An external script's instance is its own module's: the carrier holds
    // no public contract for it, so nothing is declared here.
    let declarations = (declarations && !forwards_external_default).then(|| {
        project_vue_declarations(source, canonical_id, &public, &capture, &declaration_extras)
    });
    if let (Some(external), Some(normal)) = (external, blocks.normal) {
        ops.extend(external_script_imports(
            external,
            normal.start,
            blocks.setup.is_none(),
        ));
    }
    let emission = emit_module(source, &ops, scope.as_ref(), &plan, canonical_id)?;

    let mut recovery = IncompleteProjectionState::complete();
    if let Some(scope) = &template {
        for &(state, range) in &scope.markup {
            recovery.mark_region(ProjectionRegion::Markup, state, (range.start, range.end));
        }
    }
    let (obligations, mut withheld) = match template {
        Some(_) if forwards_external_default => (
            Vec::new(),
            vec![WithheldProduct {
                family: ObligationFamily::Scaffold,
                reason: WithheldReason::Pending,
            }],
        ),
        Some(scope) => (
            scope.lowered.obligations(emission.mapping()),
            scope.withheld,
        ),
        None => (Vec::new(), Vec::new()),
    };
    // An external template's text is not part of the carrier: nothing of it
    // is checked here, and the module says so.
    if external.is_some_and(|binding| {
        binding
            .blocks
            .iter()
            .any(|block| block.kind == ExternalBlockKind::Template)
    }) && !withheld
        .iter()
        .any(|product| product.family == ObligationFamily::Scaffold)
    {
        withheld.push(WithheldProduct {
            family: ObligationFamily::Scaffold,
            reason: WithheldReason::Pending,
        });
    }
    Ok(CheckingModule {
        dialect,
        snapshot: plan.snapshot.clone(),
        emission,
        obligations,
        recovery,
        withheld,
        declarations,
    })
}

/// Apply the emit operations and the template scope to the carrier and bind
/// the result to the plan.
fn emit_module(
    source: &str,
    ops: &[EmitOp<'static>],
    scope: Option<&ScopeReplacement>,
    plan: &ProjectionPlan,
    canonical_id: &str,
) -> Result<ProjectionEmission, CheckingModuleRefusal> {
    let allocator = Allocator::default();
    let mut output = EditRecorder::new(&allocator);
    for op in ops {
        emit_op(&mut output, op);
    }
    if let Some(scope) = scope {
        if scope.range.start == scope.range.end {
            // Nothing is removed at the anchor: the pieces are ordered
            // insertions there, each anchored piece standing at its
            // authored point.
            let at = scope.range.start;
            let mut cursor = 0;
            for anchor in &scope.anchors {
                let start = anchor.content_offset as usize;
                let end = start + anchor.length as usize;
                if cursor < start {
                    emit_op(
                        &mut output,
                        &insert(at, scope.text[cursor..start].to_string()),
                    );
                }
                emit_op(
                    &mut output,
                    &EmitOp::InsertMapped {
                        at: SourceByteOffset(at),
                        text: EmitText::Owned(scope.text[start..end].to_string()),
                        source_start: SourceByteOffset(anchor.source_pos),
                        content_offset: GeneratedByteLen(0),
                    },
                );
                cursor = end;
            }
            if cursor < scope.text.len() {
                emit_op(&mut output, &insert(at, scope.text[cursor..].to_string()));
            }
        } else {
            output.overwrite_segmented(
                scope.range.start,
                scope.range.end,
                &scope.text,
                &scope.anchors,
            );
        }
    }
    let mut transform = CodeTransform::new(source, &allocator);
    output
        .apply(&mut transform)
        .map_err(CheckingModuleRefusal::Transform)?;
    ProjectionEmission::from_plan(&transform, plan, canonical_id, Vec::new())
        .map_err(CheckingModuleRefusal::Emission)
}

/// Compose a carrier whose script blocks do not parse cleanly. The recovery
/// state owns the syntax diagnostics and the checking text inserted at the
/// holes and block ends; the blocks are otherwise projected verbatim. The
/// script owners need a clean parse to build the setup scope, so the template
/// is withheld and listed rather than checked against a guessed scope.
fn compose_recovered_script(
    input: &CheckingModuleInput<'_>,
    refusal: SetupProjectionRefusal,
) -> Result<CheckingModule, CheckingModuleRefusal> {
    let CheckingModuleInput {
        source,
        artifact,
        canonical_id,
        ..
    } = *input;
    let backend = VueProjectionBackend;
    let parsed = backend
        .admitted_parse(source, artifact)
        .map_err(CheckingModuleRefusal::Script)?;
    let dialect = dialect_of(parsed);
    let regions = artifact.script_regions();
    let blocks: Vec<RecoveryBlock<'_>> = regions
        .iter()
        .filter_map(|region| {
            let region_kind = match region.kind {
                ScriptRegionKind::Module => ProjectionRegion::ModuleScope,
                ScriptRegionKind::Instance => ProjectionRegion::InstanceScope,
                _ => return None,
            };
            Some(RecoveryBlock {
                region: region_kind,
                content: source.get(region.span.start as usize..region.span.end as usize)?,
                content_start: region.span.start,
                // JavaScript, like TSX, admits JSX.
                tsx: dialect != CheckingDialect::Ts,
            })
        })
        .collect();
    let generic = parsed
        .script_setup()
        .and_then(|setup| setup.generic)
        .and_then(|span| {
            Some((
                source.get(span.start as usize..span.end as usize)?,
                span.start,
            ))
        });
    let recovery = recover_script_regions(&blocks, generic);
    if recovery.is_complete() {
        // The script owners refused what recovery finds clean: there is no
        // recovered text to check, so the refusal stands.
        return Err(CheckingModuleRefusal::Script(refusal));
    }
    let plan = backend.projection_plan(source, artifact, canonical_id);
    let block_of = |region: ProjectionRegion| {
        blocks
            .iter()
            .find(|block| block.region == region)
            .filter(|_| {
                recovery
                    .region(region)
                    .is_none_or(|marked| marked.state != RegionState::Omitted)
            })
            .map(|block| SourceRange {
                start: block.content_start,
                end: block.content_start + block.content.len() as u32,
            })
    };
    let layout = ScriptBlocks {
        normal: block_of(ProjectionRegion::ModuleScope),
        setup: block_of(ProjectionRegion::InstanceScope),
    };
    let mut ops: Vec<EmitOp<'static>> = recovery
        .script_insertions()
        .into_iter()
        .map(|(at, text)| insert(at, text.to_string()))
        .collect();
    // Retained blocks end on a line break so the next one starts a statement.
    ops.extend(
        [layout.normal, layout.setup]
            .into_iter()
            .flatten()
            .map(|block| {
                insert(
                    block.end,
                    "
"
                    .to_string(),
                )
            }),
    );
    ops.extend(layout.gaps(source.len() as u32).into_iter().map(delete));
    let emission = emit_module(source, &ops, None, &plan, canonical_id)?;
    let withheld = if parsed.template_ast().is_some() {
        vec![WithheldProduct {
            family: ObligationFamily::Scaffold,
            reason: WithheldReason::Incomplete,
        }]
    } else {
        Vec::new()
    };
    Ok(CheckingModule {
        dialect,
        snapshot: plan.snapshot.clone(),
        emission,
        obligations: Vec::new(),
        recovery,
        withheld,
        // A carrier with syntax errors publishes nothing.
        declarations: None,
    })
}

/// The legality of the first external block the projection cannot admit, in
/// carrier order. The host locates the block's `src` value from the binding.
fn first_refused_block(binding: &ExternalBlockProjectionBinding) -> Option<ExternalLegality> {
    binding
        .blocks
        .iter()
        .map(|block| &block.legality)
        .find(|legality| **legality != ExternalLegality::Admitted)
        .cloned()
}

/// The external script block as re-exports at the empty content of its
/// block: the external file is its own module, checked once wherever it is
/// named, and this module forwards its names. A module with a setup block
/// has its own default export, so only a carrier without one forwards the
/// external default. The specifier is the authored text, mapped to the `src`
/// value so a failure to resolve it lands there; one the literal must escape
/// is emitted escaped and unmapped.
fn external_script_imports(
    binding: &ExternalBlockProjectionBinding,
    at: u32,
    forwards_default: bool,
) -> Vec<EmitOp<'static>> {
    let mut ops = Vec::new();
    for block in &binding.blocks {
        if block.kind != ExternalBlockKind::Script {
            continue;
        }
        let mut escaped = String::new();
        escape_js_string_into(&mut escaped, &block.specifier);
        let mut statements = vec!["export * from"];
        if forwards_default {
            statements.push("export { default } from");
        }
        for (index, statement) in statements.into_iter().enumerate() {
            ops.push(insert(at, format!("{statement} \"")));
            // The first statement carries the mapping; a second would map
            // the same authored bytes twice.
            if index == 0 && escaped == block.specifier {
                ops.push(EmitOp::InsertMapped {
                    at: SourceByteOffset(at),
                    text: EmitText::Owned(block.specifier.clone()),
                    source_start: SourceByteOffset(block.specifier_range.start),
                    content_offset: GeneratedByteLen(0),
                });
            } else {
                ops.push(insert(at, escaped.clone()));
            }
            ops.push(insert(at, "\";\n".to_string()));
        }
    }
    ops
}

/// What the template scope's products read, all from the one admitted parse.
struct TemplateProducts<'p> {
    source: &'p str,
    artifact: &'p FrameworkParseArtifact,
    custom_elements: Option<&'p [String]>,
    plan: &'p ProjectionPlan,
    attributes: &'p AttributeOperationsProjection,
    uses: &'p ComponentUseProjection,
    script: &'p ScriptProjectionFacts,
    public: &'p VuePublicConstructorContract,
    resolution: &'p ComponentResolutionProjection,
    builtins: &'p BuiltinProjection,
    /// The carrier is JavaScript: the scope is written in its JavaScript form.
    javascript: bool,
}

/// The composed template scope: its lowered statements and the products
/// that contributed nothing.
struct TemplateScope {
    lowered: LoweredScope,
    withheld: Vec<WithheldProduct>,
    /// Template regions the topology does not mark complete, with the
    /// carrier range of each.
    markup: Vec<(RegionState, SourceRange)>,
    declaration_extras: PublicDeclarationExtras,
}

/// One statement of the template scope with its obligation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScopeStatement {
    pub(crate) family: ObligationFamily,
    pub(crate) host: Option<UseHost>,
    pub(crate) fragment: CheckingFragment,
}

/// Compose the template scope: the context view, then the template root
/// block. Every product entry point is called; a product that returns its
/// pending value, or is incomplete, is withheld and contributes nothing.
fn compose_template_scope(
    products: &TemplateProducts<'_>,
    parsed: &ParsedSfc,
    ast: &TemplateAst,
) -> Result<TemplateScope, CheckingModuleRefusal> {
    let TemplateProducts {
        source,
        artifact,
        custom_elements,
        plan,
        attributes,
        uses,
        script,
        public,
        resolution,
        builtins,
        javascript,
    } = *products;
    let backend = VueProjectionBackend;
    let script_refusal = CheckingModuleRefusal::Script;
    let kinds = setup_binding_kinds(source, parsed);
    let views = backend
        .binding_views_over(source, artifact, &kinds)
        .map_err(script_refusal)?;
    let transport = project_event_transport(plan, attributes);
    let error_spans: Vec<Span> = parsed
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.breaks_template_structure())
        .map(|diagnostic| diagnostic.span)
        .collect();
    let topology = build_template_topology(
        source,
        ast,
        TopologyRecoveryFacts {
            error_spans: &error_spans,
        },
        &mut CompilerWorkLedger::<Uncaptured>::new(),
    );

    let mut withheld = Vec::new();
    let mut withhold = |family: ObligationFamily, pending: bool, complete: bool| {
        let reason = if pending {
            WithheldReason::Pending
        } else if !complete {
            WithheldReason::Incomplete
        } else {
            return;
        };
        if !withheld
            .iter()
            .any(|known: &WithheldProduct| known.family == family)
        {
            withheld.push(WithheldProduct { family, reason });
        }
    };
    if !plan.is_complete() {
        withhold(ObligationFamily::Interpolation, false, false);
    }
    let Ok(topology) = topology else {
        // No trustworthy structure: nothing of the template is checked.
        withhold(ObligationFamily::Scaffold, false, false);
        return Ok(TemplateScope {
            lowered: LoweredScope::default(),
            withheld,
            markup: Vec::new(),
            declaration_extras: PublicDeclarationExtras::default(),
        });
    };

    let markup: Vec<(RegionState, SourceRange)> = topology
        .regions()
        .filter_map(|region| {
            let state = match topology.region(region).completeness {
                RegionCompleteness::Complete => return None,
                RegionCompleteness::Recovered => RegionState::Recovered,
                RegionCompleteness::Incomplete => RegionState::Omitted,
            };
            let span = topology.span(topology.region_owner(region).unwrap_or(topology.root()));
            Some((
                state,
                SourceRange {
                    start: span.start,
                    end: span.end,
                },
            ))
        })
        .collect();
    let branches = project_template_branches(plan, &topology);
    let iteration = project_iteration(plan, &branches);
    let reentry = project_callback_reentry(plan, &branches);
    let listeners = project_event_payloads(plan, &branches, uses, &transport, &reentry);
    let models = project_models(plan, &branches, uses, &views, &reentry);
    let provided = project_provided_slots(plan, &branches, uses);
    let outlets = project_slot_outlets(plan, &branches, &provided, attributes, public);
    let native = project_native_elements(plan, &branches, &reentry, custom_elements);
    let native_models =
        project_native_models_with_reentry(plan, &branches, &native, &views, &reentry);
    let component_uses =
        place_component_uses(source, plan, &branches, resolution, builtins, &reentry);
    let mut directives = project_directives(
        plan,
        &branches,
        uses,
        &native,
        script,
        DirectiveSyntax {
            source,
            ast,
            binding_kinds: &kinds,
        },
    );
    directives.retain_component_hosts(
        component_uses
            .statements
            .iter()
            .filter_map(|statement| statement.host.as_ref()),
    );
    let fallthrough =
        project_fallthrough(plan, &branches, resolution, public, builtins, attributes);
    let refs = project_public_instance(plan, &branches, uses, &native, public, script);
    let products: &[&dyn ComposedProduct] = &[
        &branches,
        &iteration,
        &listeners,
        &models,
        &provided,
        &outlets,
        &native,
        &native_models,
        &directives,
        builtins,
        &fallthrough,
        &refs,
    ];
    for product in withheld_products(products) {
        withhold(
            product.family,
            product.reason == WithheldReason::Pending,
            false,
        );
    }
    let uses_complete = uses.complete && resolution.complete;
    withhold(
        ObligationFamily::ComponentUse,
        false,
        uses_complete && component_uses.complete,
    );
    let (mut preludes, placed, dropped) = product_contributions(products, &component_uses);
    for family in dropped {
        withhold(family, false, false);
    }
    let placed = PlacedStatements::of(placed);
    let mut statements = Vec::new();
    if let Some(root) = branches.root() {
        emit_block(&branches, products, root, &placed, &mut statements);
    }

    let referenced = referenced_names(source, plan);
    let mut context_script = script.clone();
    context_script
        .module
        .normal_script_bindings
        .retain(|name| public.is_normal_value_binding(name));
    let context = project_template_context(
        &context_script,
        public,
        &referenced,
        &kinds,
        fallthrough.declaration_extras().reads_setup_return,
    );
    let reads_context = statements.iter().any(|statement| {
        statement.fragment.pieces.iter().any(|piece| match piece {
            CheckingPiece::Expression(id) | CheckingPiece::Replay(id) => {
                !plan.context_references(id).is_empty()
            }
            _ => false,
        })
    });
    // The template scope follows the last setup statement, which may end
    // without a terminator or in a line comment.
    let mut scope = vec![ScopeStatement {
        family: ObligationFamily::Scaffold,
        host: None,
        fragment: CheckingFragment::synth("\n"),
    }];
    if reads_context || !context.bindings.is_empty() {
        preludes.push(TEMPLATE_CONTEXT_PRELUDE);
        scope.push(ScopeStatement {
            family: ObligationFamily::Scaffold,
            host: None,
            fragment: if reads_context {
                context.statement()
            } else {
                context.unread_statement()
            },
        });
    }
    if !statements.is_empty() {
        // The template root is a block of its own inside the setup scope.
        scope.push(ScopeStatement {
            family: ObligationFamily::Scaffold,
            host: None,
            fragment: CheckingFragment::synth("{\n"),
        });
        scope.extend(statements);
        scope.push(ScopeStatement {
            family: ObligationFamily::Scaffold,
            host: None,
            fragment: CheckingFragment::synth("}\n"),
        });
    }
    let scope = if javascript {
        match javascript_scope(&scope) {
            Some(scope) => scope,
            None => {
                // A statement without a JavaScript form: nothing of the
                // template is checked, and the module says so.
                withhold(ObligationFamily::Scaffold, false, false);
                return Ok(TemplateScope {
                    lowered: LoweredScope::default(),
                    withheld,
                    markup,
                    declaration_extras: fallthrough.declaration_extras(),
                });
            }
        }
    } else {
        scope
    };
    let mut lowered = lower_scope(source, plan, &scope)?;
    lowered.preludes = preludes;
    Ok(TemplateScope {
        lowered,
        withheld,
        markup,
        declaration_extras: fallthrough.declaration_extras(),
    })
}

/// The same foreign contract declarations serve normal defaults and uses.
const FOREIGN_COMPONENT_PRELUDE: PreludeDeclaration = PreludeDeclaration {
    key: "14-foreign-component-contract",
    text: ForeignComponentContractAdapter::DECLARATIONS,
};

/// The observation declarations after the shared foreign contract prefix.
const COMPONENT_USE_PRELUDE: PreludeDeclaration = PreludeDeclaration {
    key: "15-component-use",
    text: USE_PRELUDE
        .split_at(ForeignComponentContractAdapter::DECLARATIONS.len())
        .1,
};

/// The statements of the component uses the blocks hold.
struct PlacedUses {
    statements: Vec<PlacedStatement>,
    /// False when a held use has no witness or no composable form.
    complete: bool,
}

fn withheld_products(products: &[&dyn ComposedProduct]) -> Vec<WithheldProduct> {
    products
        .iter()
        .filter_map(|product| {
            product.withheld_reason().map(|reason| WithheldProduct {
                family: product.family(),
                reason,
            })
        })
        .collect()
}

/// Gather contributions in product order. Component witnesses precede the
/// checks that read them omit absent component witnesses.
fn product_contributions(
    products: &[&dyn ComposedProduct],
    component_uses: &PlacedUses,
) -> (
    Vec<PreludeDeclaration>,
    Vec<PlacedStatement>,
    Vec<ObligationFamily>,
) {
    let mut preludes = Vec::new();
    let mut placed = component_uses.statements.clone();
    let mut dropped = Vec::new();
    if !placed.is_empty() {
        preludes.push(FOREIGN_COMPONENT_PRELUDE);
        preludes.push(COMPONENT_USE_PRELUDE);
    }
    for product in products {
        if product.withheld_reason() == Some(WithheldReason::Pending) {
            continue;
        }
        let mut statements = product.statements();
        if matches!(
            product.family(),
            ObligationFamily::Model | ObligationFamily::TemplateRef | ObligationFamily::Listener
        ) {
            let count = statements.len();
            statements.retain(|statement| {
                (product.family() == ObligationFamily::TemplateRef
                    && !matches!(statement.host, Some(UseHost::Component(_))))
                    || component_uses
                        .statements
                        .iter()
                        .any(|placed| placed.host == statement.host)
            });
            if statements.len() != count {
                dropped.push(product.family());
            }
            if statements.is_empty() {
                continue;
            }
        }
        preludes.extend(product.preludes());
        placed.extend(statements);
    }
    (preludes, placed, dropped)
}

#[cfg(test)]
mod product_composition_tests {
    use super::*;
    use crate::ide::vue_projection::checking_fragment::TemplateBlockId;

    struct Product {
        family: ObligationFamily,
        reason: Option<WithheldReason>,
    }

    impl ComposedProduct for Product {
        fn family(&self) -> ObligationFamily {
            self.family
        }

        fn withheld_reason(&self) -> Option<WithheldReason> {
            self.reason.clone()
        }

        fn preludes(&self) -> Vec<PreludeDeclaration> {
            assert_ne!(self.reason, Some(WithheldReason::Pending));
            vec![TEMPLATE_CONTEXT_PRELUDE]
        }

        fn statements(&self) -> Vec<PlacedStatement> {
            assert_ne!(self.reason, Some(WithheldReason::Pending));
            vec![PlacedStatement {
                anchor: StatementAnchor::BlockTail(TemplateBlockId(0)),
                phase: StatementPhase::Main,
                family: self.family,
                host: None,
                fragment: CheckingFragment::synth("check();\n"),
            }]
        }
    }

    #[test]
    fn the_product_list_routes_status_and_retains_partial_contributions() {
        let checked = Product {
            family: ObligationFamily::NativeElement,
            reason: None,
        };
        let pending = Product {
            family: ObligationFamily::Builtin,
            reason: Some(WithheldReason::Pending),
        };
        let partial = Product {
            family: ObligationFamily::Directive,
            reason: Some(WithheldReason::Incomplete),
        };
        let products: &[&dyn ComposedProduct] = &[&checked, &pending, &partial];
        assert_eq!(
            withheld_products(products),
            vec![
                WithheldProduct {
                    family: ObligationFamily::Builtin,
                    reason: WithheldReason::Pending
                },
                WithheldProduct {
                    family: ObligationFamily::Directive,
                    reason: WithheldReason::Incomplete
                },
            ]
        );
        let uses = PlacedUses {
            statements: Vec::new(),
            complete: true,
        };
        let (preludes, statements, dropped) = product_contributions(products, &uses);
        assert_eq!(preludes, vec![TEMPLATE_CONTEXT_PRELUDE; 2]);
        assert_eq!(
            statements
                .iter()
                .map(|statement| statement.family)
                .collect::<Vec<_>>(),
            vec![ObligationFamily::NativeElement, ObligationFamily::Directive]
        );
        assert!(dropped.is_empty());
    }
}

/// Each component use at its item: the resolved witness's statements, then
/// a read of every binding they declare, so a use with no checked member
/// and its validation checks are not unused locals. A use written `<component :is>` reads its authored value
/// in the template context. A dynamic use whose component and props come
/// from one object is checked by a correlation over authored member paths
/// that this route does not compose yet, so it is left out and the product
/// is incomplete.
fn place_component_uses(
    source: &str,
    plan: &ProjectionPlan,
    branches: &TemplateBranchProjection,
    resolution: &ComponentResolutionProjection,
    builtins: &BuiltinProjection,
    reentry: &CallbackReentryPlan,
) -> PlacedUses {
    let mut placed = PlacedUses {
        statements: Vec::new(),
        complete: true,
    };
    let builtin_uses: FxHashMap<_, _> = builtins
        .obligations
        .iter()
        .map(|o| (&o.use_id, o))
        .collect();
    for block in &branches.blocks {
        for (index, item) in block.items.iter().enumerate() {
            let TemplateItem::Component(use_id) = item else {
                continue;
            };
            let correlated = resolution
                .dynamic
                .iter()
                .any(|dynamic| dynamic.use_id == *use_id && dynamic.correlated_source.is_some());
            let observation = resolution
                .observations
                .iter()
                .find(|observation| observation.use_id == *use_id);
            let Some(witness) = observation
                .filter(|_| !correlated)
                .map(|observation| &observation.witness)
            else {
                placed.complete = false;
                continue;
            };
            let builtin = builtin_uses.get(use_id);
            let mut witness = witness.clone();
            if let Some(builtin) = builtin {
                witness.component = builtin.constructor.clone();
            }
            let component = match plan.expression(&witness.component_expression) {
                Some(occurrence) if occurrence.kind == ExpressionKind::ComponentIs => {
                    WitnessComponent::Authored
                }
                Some(occurrence) => {
                    let (start, end) = trim_span(source, occurrence.start, occurrence.end);
                    WitnessComponent::Named {
                        tag: Some(SourceRange { start, end }),
                    }
                }
                None => WitnessComponent::Named { tag: None },
            };
            let anchor = StatementAnchor::Item {
                block: block.id,
                item: index as u32,
            };
            let host = Some(UseHost::Component(use_id.clone()));
            if let Some(builtin) = builtin {
                placed.statements.push(PlacedStatement {
                    anchor: anchor.clone(),
                    phase: StatementPhase::Before,
                    family: ObligationFamily::Builtin,
                    host: host.clone(),
                    fragment: builtin.before(),
                });
            }
            for (phase, fragment) in witness.statement_fragments_over(reentry, component) {
                placed.statements.push(PlacedStatement {
                    anchor: anchor.clone(),
                    phase,
                    family: if builtin.is_some() {
                        ObligationFamily::Builtin
                    } else {
                        ObligationFamily::ComponentUse
                    },
                    host: host.clone(),
                    fragment,
                });
            }
            placed.statements.push(PlacedStatement {
                anchor,
                phase: StatementPhase::After,
                family: ObligationFamily::Scaffold,
                host,
                fragment: CheckingFragment::synth(format!(
                    "{};\n",
                    witness.declared_bindings().join("; ")
                )),
            });
        }
    }
    placed
}

/// Placed statements grouped by anchor: `(block, Some(item))` for an item,
/// `(block, None)` for the block's tail. Each group is in phase order and,
/// within a phase, in the order its products returned them.
struct PlacedStatements {
    by_anchor: FxHashMap<(u32, Option<u32>), Vec<PlacedStatement>>,
}

impl PlacedStatements {
    fn of(statements: Vec<PlacedStatement>) -> Self {
        let mut by_anchor: FxHashMap<(u32, Option<u32>), Vec<PlacedStatement>> =
            FxHashMap::default();
        for statement in statements {
            let key = match statement.anchor {
                StatementAnchor::Item { block, item } => (block.0, Some(item)),
                StatementAnchor::BlockTail(block) => (block.0, None),
            };
            by_anchor.entry(key).or_default().push(statement);
        }
        for group in by_anchor.values_mut() {
            group.sort_by_key(|statement| statement.phase);
        }
        Self { by_anchor }
    }

    fn emit(&self, key: (u32, Option<u32>), out: &mut Vec<ScopeStatement>) {
        for statement in self.by_anchor.get(&key).into_iter().flatten() {
            out.push(ScopeStatement {
                family: statement.family,
                host: statement.host.clone(),
                fragment: statement.fragment.clone(),
            });
        }
    }
}

/// Emit `block`'s items in authored order: each item's statements, and for
/// a child block its head, its own items and its closing brace. An if chain
/// is emitted whole or not at all, so a dropped arm never turns the next
/// `else` into the else of a different condition. A loop or slot body is
/// emitted once its product opens it; until then it is left out with all
/// it holds.
fn emit_block(
    branches: &TemplateBranchProjection,
    products: &[&dyn ComposedProduct],
    block: &TemplateBlock,
    placed: &PlacedStatements,
    out: &mut Vec<ScopeStatement>,
) {
    // A region the topology marks incomplete is listed, not checked.
    if !block.complete {
        return;
    }
    let mut dropped_chain = None;
    for (index, item) in block.items.iter().enumerate() {
        placed.emit((block.id.0, Some(index as u32)), out);
        let TemplateItem::Block(child) = item else {
            continue;
        };
        let Some(child) = branches.block(*child) else {
            continue;
        };
        if let TemplateBlockKind::Arm { chain, .. } = &child.kind {
            let in_chain = |item: &TemplateItem| arm_of_chain(branches, item, *chain);
            if !block.items[..index]
                .iter()
                .any(|earlier| in_chain(earlier).is_some())
            {
                let whole = block
                    .items
                    .iter()
                    .filter_map(in_chain)
                    .all(|arm| arm.complete);
                dropped_chain = (!whole).then_some(*chain);
            }
            if dropped_chain == Some(*chain) {
                continue;
            }
        }
        let Some((family, head)) = products.iter().find_map(|product| {
            (product.withheld_reason() != Some(WithheldReason::Pending))
                .then(|| product.block_head(child))
                .flatten()
                .map(|head| (product.family(), head))
        }) else {
            continue;
        };
        if !head.before.is_empty() {
            out.push(ScopeStatement {
                family,
                host: None,
                fragment: head.before,
            });
        }
        out.push(ScopeStatement {
            family,
            host: None,
            fragment: head.open,
        });
        emit_block(branches, products, child, placed, out);
        out.push(ScopeStatement {
            family: ObligationFamily::Scaffold,
            host: None,
            fragment: CheckingFragment::synth("}\n"),
        });
    }
    placed.emit((block.id.0, None), out);
}

/// The arm `item` opens when it is a block of if chain `chain`.
fn arm_of_chain<'b>(
    branches: &'b TemplateBranchProjection,
    item: &TemplateItem,
    chain: u32,
) -> Option<&'b TemplateBlock> {
    let TemplateItem::Block(id) = item else {
        return None;
    };
    branches
        .block(*id)
        .filter(|arm| matches!(arm.kind, TemplateBlockKind::Arm { chain: of, .. } if of == chain))
}

/// The names the template's context references spell, first occurrence
/// first.
fn referenced_names(source: &str, plan: &ProjectionPlan) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for occurrence in plan.expressions() {
        let references = plan.context_references(&occurrence.id);
        let spelled = match emitted_range(source, occurrence) {
            Some(_) => references
                .iter()
                .filter_map(|reference| {
                    source.get(reference.start as usize..reference.end as usize)
                })
                .collect::<Vec<_>>(),
            None if references.is_empty() => Vec::new(),
            None => vec![occurrence.spelling.trim()],
        };
        for name in spelled {
            if !names.iter().any(|known| known == name) {
                names.push(name.to_string());
            }
        }
    }
    names
}

/// The setup bindings with the parser's classification of each, read from
/// the one script preparation lane. Props the setup does not bind locally
/// are not setup bindings and are left out.
pub(crate) fn setup_binding_kinds(source: &str, parsed: &ParsedSfc) -> Vec<(String, BindingType)> {
    let allocator = Allocator::default();
    let prepared = PreparedScript::build(source, None, parsed.script_setup(), &allocator);
    let Some(setup) = prepared.setup() else {
        return Vec::new();
    };
    let content = setup.content_str();
    setup
        .parse_result()
        .bindings
        .iter()
        // Prop spans index the carrier, every other span the setup content.
        .filter(|(_, kind)| !matches!(kind, BindingType::Props | BindingType::PropsAliased))
        .filter_map(|(span, kind)| {
            let name = content.get(span.start as usize..span.end as usize)?;
            Some((name.to_string(), *kind))
        })
        .collect()
}

/// What a piece of the lowered template scope stands for in the carrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PieceOrigin {
    /// The authored bytes at this offset, copied unchanged: every byte of
    /// the piece corresponds to one authored byte.
    Exact(u32),
    /// Text that stands at this authored offset without copying it (a
    /// generated spelling of an authored name).
    Point(u32),
}

/// One piece of the lowered template scope: its text and, for authored
/// bytes, what it stands for. Scaffolding has no origin.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ScopePiece {
    text: String,
    origin: Option<PieceOrigin>,
}

/// The template scope written over the carrier bytes the module removes at
/// its anchor. One replacement keeps the pieces in order and lets each
/// authored piece keep its exact correspondence.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ScopeReplacement {
    range: SourceRange,
    text: String,
    anchors: Vec<SegmentAnchor>,
}

/// One statement's place in the lowered run.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ScopeSpan {
    start: u32,
    end: u32,
    family: ObligationFamily,
    host: Option<UseHost>,
}

/// The template scope lowered to one run of pieces inserted at a single
/// anchor, with each statement's span in the run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LoweredScope {
    pieces: Vec<ScopePiece>,
    spans: Vec<ScopeSpan>,
    preludes: Vec<PreludeDeclaration>,
    run_len: u32,
}

impl LoweredScope {
    /// The run as one replacement text between unmapped `before` and
    /// `after`, with an exact anchor for each authored piece and a point
    /// anchor for each piece that stands at an authored offset.
    fn segmented(&self, before: &str, after: &str) -> (String, Vec<SegmentAnchor>) {
        let mut text = String::with_capacity(before.len() + self.run_len as usize + after.len());
        text.push_str(before);
        let mut anchors = Vec::new();
        for piece in &self.pieces {
            if let Some(origin) = piece.origin {
                let (source_pos, kind) = match origin {
                    PieceOrigin::Exact(at) => (at, AnchorKind::Exact),
                    PieceOrigin::Point(at) => (at, AnchorKind::Point),
                };
                anchors.push(SegmentAnchor {
                    content_offset: text.len() as u32,
                    length: piece.text.len() as u32,
                    source_pos,
                    kind,
                });
            }
            text.push_str(&piece.text);
        }
        text.push_str(after);
        (text, anchors)
    }

    /// Statement spans in the checking text. The run is placed by its first
    /// authored piece, the first projected region that starts at that
    /// piece's origin; a run with no authored piece reads no template
    /// expression and has no span.
    fn obligations(&self, mapping: &MappingProduct) -> Vec<ObligationSpan> {
        let mut offset = 0u32;
        let mut placed = None;
        for piece in &self.pieces {
            let len = piece.text.len() as u32;
            if let Some(origin) = piece.origin {
                placed = mapping
                    .projected()
                    .iter()
                    .find(|region| {
                        let generated = region.generated.end - region.generated.start;
                        let (PieceOrigin::Exact(at) | PieceOrigin::Point(at)) = origin;
                        // Exact bytes coalesce with the exact bytes that
                        // follow them, never with the scaffolding before. An
                        // inserted scope stands every piece at a point.
                        let exact = matches!(origin, PieceOrigin::Exact(_))
                            && matches!(
                                region.class,
                                ProjectedClass::Identity | ProjectedClass::Relocated
                            )
                            && region.carrier.is_some_and(|carrier| carrier.start == at)
                            && generated >= len;
                        let point = region.class == ProjectedClass::Rewritten
                            && region.carrier == Some(MappingSpan { start: at, end: at })
                            && generated == len;
                        exact || point
                    })
                    .map(|region| region.generated.start - offset);
                break;
            }
            offset += len;
        }
        let Some(base) = placed else {
            return Vec::new();
        };
        self.spans
            .iter()
            .map(|span| ObligationSpan {
                generated: SourceRange {
                    start: base + span.start,
                    end: base + span.end,
                },
                family: span.family,
                host: span.host.clone(),
            })
            .collect()
    }
}

/// The JavaScript form of the template scope. Each statement keeps its
/// family, host and authored pieces; `None` when a statement has no
/// JavaScript form.
fn javascript_scope(scope: &[ScopeStatement]) -> Option<Vec<ScopeStatement>> {
    let fragments: Vec<CheckingFragment> = scope
        .iter()
        .map(|statement| statement.fragment.clone())
        .collect();
    let rendered = javascript_statements(&fragments).ok()?;
    Some(
        scope
            .iter()
            .zip(rendered)
            .map(|(statement, fragment)| ScopeStatement {
                family: statement.family,
                host: statement.host.clone(),
                fragment,
            })
            .collect(),
    )
}

/// Lower statements to one run. An expression is emitted as `Expression` at
/// most once per module, with its context references qualified; a `Replay`
/// is the same text with the same correspondence, so a diagnostic it repeats
/// lands on the same authored bytes as the original's.
pub(crate) fn lower_scope(
    source: &str,
    plan: &ProjectionPlan,
    statements: &[ScopeStatement],
) -> Result<LoweredScope, CheckingModuleRefusal> {
    let mut lowered = LoweredScope::default();
    let mut emitted: Vec<&AdmittedExpressionId> = Vec::new();
    for statement in statements {
        let start = lowered.run_len();
        for piece in &statement.fragment.pieces {
            match piece {
                CheckingPiece::Synth(text) => lowered.push(text.clone(), None),
                CheckingPiece::Verbatim(range) => {
                    let text = source
                        .get(range.start as usize..range.end as usize)
                        .unwrap_or_default();
                    lowered.push(text.to_string(), Some(PieceOrigin::Exact(range.start)));
                }
                CheckingPiece::Alias { text, source } => {
                    lowered.push(text.clone(), Some(PieceOrigin::Point(source.start)));
                }
                CheckingPiece::Expression(id) => {
                    if emitted.contains(&id) {
                        return Err(CheckingModuleRefusal::FragmentInvariant {
                            family: statement.family,
                            detail: format!("expression {} is emitted twice", id.digest_hex()),
                        });
                    }
                    emitted.push(id);
                    for (text, carrier) in qualified(source, plan, id, statement.family)? {
                        lowered.push(text, carrier);
                    }
                }
                CheckingPiece::Replay(id) => {
                    for (text, origin) in qualified(source, plan, id, statement.family)? {
                        lowered.push(text, origin);
                    }
                }
            }
        }
        lowered.spans.push(ScopeSpan {
            start,
            end: lowered.run_len(),
            family: statement.family,
            host: statement.host.clone(),
        });
    }
    Ok(lowered)
}

impl LoweredScope {
    /// Byte length of the run so far.
    fn run_len(&self) -> u32 {
        self.run_len
    }

    fn push(&mut self, text: String, origin: Option<PieceOrigin>) {
        if !text.is_empty() {
            self.run_len += text.len() as u32;
            self.pieces.push(ScopePiece { text, origin });
        }
    }
}

/// The authored bytes an expression is emitted from: its range without
/// surrounding whitespace or the statement terminator a template value may
/// end in. `None` when the plan spells the expression differently from
/// those bytes (a value-less `:foo-bar` reads `fooBar`).
fn emitted_range(source: &str, occurrence: &ExpressionOccurrence) -> Option<(u32, u32)> {
    let (start, end) = trim_span(source, occurrence.start, occurrence.end);
    let authored = trim_handler_body(source.get(start as usize..end as usize)?);
    (authored == trim_handler_body(occurrence.spelling.trim()))
        .then_some((start, start + authored.len() as u32))
}

/// The authored expression with each context reference read through the
/// template context, as pieces paired with what each stands for: authored
/// pieces are exact copies, the qualifiers are scaffolding. A shorthand
/// property keeps its key: `{ login }` reads `{ login: __VerterCtx.login }`.
fn qualified(
    source: &str,
    plan: &ProjectionPlan,
    id: &AdmittedExpressionId,
    family: ObligationFamily,
) -> Result<Vec<(String, Option<PieceOrigin>)>, CheckingModuleRefusal> {
    let occurrence =
        plan.expression(id)
            .ok_or_else(|| CheckingModuleRefusal::FragmentInvariant {
                family,
                detail: format!("expression {} is not in the plan", id.digest_hex()),
            })?;
    let Some((start, end)) = emitted_range(source, occurrence) else {
        // A generated spelling of an authored name stands at the name.
        let mut pieces = Vec::new();
        if !plan.context_references(id).is_empty() {
            pieces.push((format!("{TEMPLATE_CONTEXT}."), None));
        }
        pieces.push((
            occurrence.spelling.trim().to_string(),
            Some(PieceOrigin::Point(
                trim_span(source, occurrence.start, occurrence.end).0,
            )),
        ));
        return Ok(pieces);
    };
    let mut pieces = Vec::new();
    let mut cursor = start;
    for reference in plan.context_references(id) {
        if reference.start < cursor || reference.end > end {
            continue;
        }
        if cursor < reference.start {
            pieces.push((
                source[cursor as usize..reference.start as usize].to_string(),
                Some(PieceOrigin::Exact(cursor)),
            ));
        }
        let name = &source[reference.start as usize..reference.end as usize];
        let qualifier = if reference.shorthand {
            format!("{name}: {TEMPLATE_CONTEXT}.")
        } else {
            format!("{TEMPLATE_CONTEXT}.")
        };
        pieces.push((qualifier, None));
        pieces.push((name.to_string(), Some(PieceOrigin::Exact(reference.start))));
        cursor = reference.end;
    }
    if cursor < end {
        pieces.push((
            source[cursor as usize..end as usize].to_string(),
            Some(PieceOrigin::Exact(cursor)),
        ));
    }
    Ok(pieces)
}

/// Carrier ranges of the authored script contents.
struct ScriptBlocks {
    normal: Option<SourceRange>,
    setup: Option<SourceRange>,
}

impl ScriptBlocks {
    /// Carrier ranges outside every authored script content, in order.
    fn gaps(&self, source_len: u32) -> Vec<SourceRange> {
        let mut kept: Vec<SourceRange> = [self.normal, self.setup].into_iter().flatten().collect();
        kept.sort_by_key(|range| range.start);
        kept.push(SourceRange {
            start: source_len,
            end: source_len,
        });
        let mut gaps = Vec::new();
        let mut cursor = 0;
        for range in kept {
            if cursor < range.start {
                gaps.push(SourceRange {
                    start: cursor,
                    end: range.start,
                });
            }
            cursor = cursor.max(range.end);
        }
        gaps
    }
}

fn byte_range(range: SourceRange) -> SourceByteRange {
    SourceByteRange::new(SourceByteOffset(range.start), SourceByteOffset(range.end))
}

fn insert(at: u32, text: String) -> EmitOp<'static> {
    EmitOp::InsertUnmapped {
        at: SourceByteOffset(at),
        text: EmitText::Owned(text),
    }
}

fn delete(range: SourceRange) -> EmitOp<'static> {
    EmitOp::OverwriteSyntheticBoundary {
        source: byte_range(range),
        text: EmitText::Static(""),
        anchor: None,
    }
}

/// The module layout over the carrier, as emit operations and the template
/// scope's replacement (see `scope_at`):
///
/// - the normal script, the setup imports and the setup type declarations
///   that may stand at module scope (see `module_scope_types`), verbatim
///   (the setup ones moved ahead of the setup scope, in source order, each
///   with its own line end, an exported one keeping its `export`; an
///   exported one that must stay in the setup scope loses it there, where
///   it is no legal modifier);
/// - the preludes, each once in key order, the global component fallbacks
///   and the public declaration over the setup scope;
/// - one function over the authored generic binder holding the remaining
///   setup statements in place, then the template scope, with the
///   `defineExpose` value bound so the function returns it.
///
/// A carrier without setup receives a scope for its template after the normal
/// script. A scriptless carrier receives the same scope and an empty constructor.
///
/// Every byte outside the script contents is removed; the removed bytes at
/// the template scope's anchor are replaced by the scope. Hoisted statements
/// move to the setup contents' start: moves apply before insertions, and an
/// insertion at a position lands after the moved text already there, so the
/// hoisted statements keep their source order and precede the synthesized
/// declarations.
#[allow(
    clippy::too_many_arguments,
    reason = "Layout combines authored source and shared script, public-surface and type facts without cloning those inputs."
)]
fn module_layout(
    source: &str,
    blocks: &ScriptBlocks,
    shape: CarrierShape,
    script: &ScriptProjectionFacts,
    public: &VuePublicConstructorContract,
    capture: &BinderCapturePlan,
    fallbacks: &str,
    template: &LoweredScope,
    extras: &PublicDeclarationExtras,
    dialect: CheckingDialect,
) -> Result<(Vec<EmitOp<'static>>, ScopeReplacement), CheckingModuleRefusal> {
    let mut ops = public.normal_script_edits();
    let mut gaps = blocks.gaps(source.len() as u32);
    // A JavaScript module's helper declarations and where they go.
    let mut hoisted = None;
    let helpers_at = [blocks.normal, blocks.setup]
        .into_iter()
        .flatten()
        .map(|block| block.start)
        .min();
    let pragma = script.module.file_pragma;
    let declaration = public.declaration_over(SETUP_SCOPE, extras);
    let scope = match (shape, blocks.setup, &script.setup, declaration) {
        (
            CarrierShape::ScriptSetup | CarrierShape::ScriptAndSetup,
            Some(setup),
            Some(facts),
            Some(declaration),
        ) => {
            let module_types = module_scope_types(capture, &script.module.normal_script_bindings);
            for (ordinal, statement) in facts.statements.iter().enumerate() {
                let hoisted = match &statement.kind {
                    SetupStatementKind::Import { .. } => true,
                    // An exported type is judged as any other: one that
                    // reads a setup value or a binder parameter, which only
                    // the setup scope binds, stays there without `export`.
                    SetupStatementKind::TypeDeclaration { name, export } => {
                        let hoisted = module_types.contains(name.as_str());
                        if let (false, Some(export)) = (hoisted, export) {
                            ops.push(delete(*export));
                        }
                        hoisted
                    }
                    SetupStatementKind::Declaration { .. } | SetupStatementKind::Other => false,
                };
                if hoisted {
                    let next = facts
                        .statements
                        .get(ordinal + 1)
                        .map_or(setup.end, |next| next.span.start);
                    ops.push(EmitOp::MoveOriginal {
                        source: byte_range(with_line_end(source, statement.span, next)),
                        at: SourceByteOffset(setup.start),
                    });
                }
            }
            let exposed = public
                .expose_callee()
                .and_then(|callee| exposed_binding(facts, callee));
            let mut preludes = template.preludes.clone();
            if public.uses_foreign_constructor() {
                preludes.push(FOREIGN_COMPONENT_PRELUDE);
            }
            if exposed.is_some() {
                preludes.push(EXPOSED_VALUE_PRELUDE);
            }
            preludes.sort_by_key(|prelude| prelude.key);
            preludes.dedup_by_key(|prelude| prelude.key);
            let mut helpers = String::from("\n");
            for prelude in &preludes {
                helpers.push_str(prelude.text);
            }
            helpers.push_str(fallbacks);
            let mut module_scope = module_scope_in(
                helpers,
                &declaration,
                dialect,
                helpers_at,
                pragma,
                &mut hoisted,
            )?;
            let asyncness = if public.setup_is_async() {
                "async "
            } else {
                ""
            };
            module_scope.push_str(&format!(
                "{asyncness}function {SETUP_SCOPE}{}() {{",
                render_binder_list(&public.binder, BinderSite::Construct)
            ));
            ops.push(insert(setup.start, module_scope));
            let mut close = String::from("\n");
            if let Some(exposed) = &exposed {
                if let Some(at) = exposed.bind_at {
                    ops.push(insert(at, format!("const {EXPOSED_BINDING} = ")));
                }
                ops.push(delete(exposed.callee));
                ops.push(insert(exposed.callee.start, EXPOSED_VALUE.to_string()));
                close.push_str(&format!("return {};\n", exposed.returned));
            }
            close.push_str(&format!("}}\nvoid {SETUP_SCOPE};\n"));
            // The template scope follows the setup statements, before the
            // exposed value is returned.
            scope_at(&mut gaps, setup.end, template, "", &close)
        }
        (_, _, _, declaration) => {
            let at = blocks
                .normal
                .map_or(source.len() as u32, |normal| normal.end);
            let mut helpers = String::from("\n");
            let mut preludes = template.preludes.clone();
            if public.uses_foreign_constructor() {
                preludes.push(FOREIGN_COMPONENT_PRELUDE);
            }
            preludes.sort_by_key(|prelude| prelude.key);
            preludes.dedup_by_key(|prelude| prelude.key);
            for prelude in preludes {
                helpers.push_str(prelude.text);
            }
            helpers.push_str(fallbacks);
            let mut module_scope = module_scope_in(
                helpers,
                declaration.as_deref().unwrap_or_default(),
                dialect,
                helpers_at,
                pragma,
                &mut hoisted,
            )?;
            module_scope.push_str(&format!("function {SETUP_SCOPE}() {{\n"));
            scope_at(
                &mut gaps,
                at,
                template,
                &module_scope,
                &format!("\n}}\nvoid {SETUP_SCOPE};\n"),
            )
        }
    };
    // Hoisted helpers take the place of the deleted text before the first
    // script block, so no rewrite of the block's first bytes precedes them.
    for gap in gaps {
        match hoisted.take() {
            Some((first, text)) if gap.end == first => {
                ops.push(EmitOp::OverwriteSyntheticBoundary {
                    source: byte_range(gap),
                    text: EmitText::Owned(text),
                    anchor: None,
                });
            }
            other => {
                hoisted = other;
                ops.push(delete(gap));
            }
        }
    }
    if let Some((first, text)) = hoisted {
        ops.insert(0, insert(first, text));
    }
    Ok((ops, scope))
}

/// The template scope at `at`, between unmapped `before` and `after`,
/// written over the removed carrier bytes that begin at `at` (or, in a
/// carrier with no script, end there). That range is taken out of `gaps`:
/// the replacement removes it. A carrier that removes nothing at `at` (an
/// empty one, or one whose script block runs to its end without a closing
/// tag) has the scope inserted there instead, its authored pieces standing
/// at authored points.
fn scope_at(
    gaps: &mut Vec<SourceRange>,
    at: u32,
    template: &LoweredScope,
    before: &str,
    after: &str,
) -> ScopeReplacement {
    let (text, anchors) = template.segmented(before, after);
    let index = gaps
        .iter()
        .position(|gap| gap.start == at)
        .or_else(|| gaps.iter().position(|gap| gap.end == at));
    let range = match index {
        Some(index) => gaps.remove(index),
        None => SourceRange { start: at, end: at },
    };
    ScopeReplacement {
        range,
        text,
        anchors,
    }
}

/// The module-scope text: the helper declarations, then the public
/// declaration. A TypeScript module keeps both in place. A JavaScript module
/// carries their JavaScript form, where a helper is a `var` read only after
/// its assignment, so the helpers go to `helpers_at`, ahead of the authored
/// script whose Options tail calls them; the public declaration stays in
/// place.
fn module_scope_in(
    helpers: String,
    declaration: &str,
    dialect: CheckingDialect,
    helpers_at: Option<u32>,
    pragma: Option<FilePragma>,
    hoisted: &mut Option<(u32, String)>,
) -> Result<String, CheckingModuleRefusal> {
    if !dialect.is_javascript() {
        return Ok(helpers + declaration);
    }
    let Some(at) = helpers_at else {
        return declarations_in(helpers + declaration, dialect);
    };
    // The module opens with the scripts' file pragma, where TypeScript (and
    // vue-tsc) read it.
    let mut helpers = format!(
        "{}{}",
        pragma.map_or("", FilePragma::line),
        declarations_in(helpers, dialect)?
    );
    helpers.push('\n');
    *hoisted = Some((at, helpers));
    declarations_in(format!("\n{declaration}"), dialect)
}

/// Module-scope declarations in the module's dialect: a JavaScript module
/// carries their JavaScript form.
fn declarations_in(
    text: String,
    dialect: CheckingDialect,
) -> Result<String, CheckingModuleRefusal> {
    if !dialect.is_javascript() {
        return Ok(text);
    }
    javascript_declarations(&text).map_err(|NoJavaScriptForm(detail)| {
        CheckingModuleRefusal::FragmentInvariant {
            family: ObligationFamily::Scaffold,
            detail: format!("a generated declaration has no JavaScript form: {detail}"),
        }
    })
}

/// `span` through the end of its line when that line ends before `next`, so
/// a moved statement keeps the line break that separates it from the next.
fn with_line_end(source: &str, span: SourceRange, next: u32) -> SourceRange {
    let trailing = &source[span.end as usize..next.max(span.end) as usize];
    match trailing.find('\n') {
        Some(at) => SourceRange {
            start: span.start,
            end: span.end + at as u32 + 1,
        },
        None => span,
    }
}

/// How the setup scope captures the `defineExpose` value it returns.
struct ExposedBinding {
    /// The authored callee, replaced with the identity wrapper.
    callee: SourceRange,
    /// Where `const <binding> = ` opens the binding; `None` when the call
    /// initializes an authored declaration already.
    bind_at: Option<u32>,
    /// The name the scope returns.
    returned: String,
}

/// The binding for a `defineExpose` call in a macro position: an expression
/// statement (possibly wrapped in parentheses or an assertion) is bound as a
/// whole; the initializer of a single authored declaration is returned
/// through that declaration's name.
fn exposed_binding(facts: &TsSetupProjection, callee: SourceRange) -> Option<ExposedBinding> {
    let statement = facts.statements.iter().find(|statement| {
        statement.span.start <= callee.start && callee.end <= statement.span.end
    })?;
    match &statement.kind {
        SetupStatementKind::Other => Some(ExposedBinding {
            callee,
            bind_at: Some(statement.span.start),
            returned: EXPOSED_BINDING.to_string(),
        }),
        SetupStatementKind::Declaration { names } if names.len() == 1 => Some(ExposedBinding {
            callee,
            bind_at: None,
            returned: names[0].clone(),
        }),
        _ => None,
    }
}

/// Names of the setup type declarations that may stand at module scope:
/// those the public declaration or an exported setup type names (the binder
/// capture's lifted declarations) that are free in no binder parameter,
/// depend on no setup value, transitively, and share no name with a
/// normal-script binding.
/// Every other setup type declaration stays in the setup scope, where the
/// binder and the setup values are in scope and where a setup declaration
/// shadows a normal-script one of the same name instead of merging with it.
fn module_scope_types<'c>(
    capture: &'c BinderCapturePlan,
    normal_script_bindings: &[String],
) -> FxHashSet<&'c str> {
    let safe = module_scope_safety(capture);
    let mut blocked = FxHashSet::default();
    let mut hoisted = FxHashSet::default();
    for (declaration, safe) in capture.declarations.iter().zip(safe) {
        if !declaration.from_setup {
            continue;
        }
        if safe {
            hoisted.insert(declaration.name.as_str());
        } else {
            blocked.insert(declaration.name.as_str());
        }
    }
    hoisted.retain(|name| {
        !blocked.contains(name) && !normal_script_bindings.iter().any(|bound| bound == name)
    });
    hoisted
}

/// Whether each lifted declaration may stand at module scope: it is a
/// normal-script declaration or a setup type free in no binder parameter,
/// and every local declaration it reaches is one too. Unsafety spreads from
/// each unsafe declaration to its dependents until none changes, so a cycle
/// is safe only when every member it reaches is, whichever member is asked
/// first.
fn module_scope_safety(capture: &BinderCapturePlan) -> Vec<bool> {
    let count = capture.declarations.len();
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); count];
    let mut safe: Vec<bool> = Vec::with_capacity(count);
    for (index, declaration) in capture.declarations.iter().enumerate() {
        let mut locally_safe = !declaration.from_setup
            || (matches!(
                declaration.kind,
                LiftedDeclarationKind::TypeAlias | LiftedDeclarationKind::Interface
            ) && declaration.binder_params.is_empty());
        for dependency in &declaration.dependencies {
            let reached: &[usize] = match &dependency.origin {
                DependencyOrigin::LocalDeclaration { index } => std::slice::from_ref(index),
                DependencyOrigin::LocalDeclarations { indices } => indices,
                DependencyOrigin::BinderParam { .. } => {
                    locally_safe = false;
                    &[]
                }
                _ => &[],
            };
            for &reached in reached.iter().filter(|&&reached| reached < count) {
                dependents[reached].push(index);
            }
        }
        safe.push(locally_safe);
    }
    let mut pending: Vec<usize> = (0..count).filter(|&index| !safe[index]).collect();
    while let Some(unsafe_index) = pending.pop() {
        for &dependent in &dependents[unsafe_index] {
            if safe[dependent] {
                safe[dependent] = false;
                pending.push(dependent);
            }
        }
    }
    safe
}
