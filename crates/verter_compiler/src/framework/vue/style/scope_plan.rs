#![allow(dead_code, unused_variables)]

use verter_audit::observe::ObserveExec;
use verter_css_syntax::{PreprocessorIdentity, QualifiedStyleResult, StyleStage};
use verter_span::Span;

use crate::assembly::emit_plan::{DecodedMap, MapMode};
use crate::assembly::map_input::validate_and_decode;
use crate::assembly::svelte_module::{generated_line_starts, utf16_offset_to_bytes};
use crate::compile::compute_scope_id;
use crate::compile_request::demand::InputDemand;
use crate::emission::{DecodedMapSegment, MapSegmentKind};
use crate::framework::vue::semantic::VueSemanticSnapshot;
use crate::framework::vue::style::cascade::{
    run_vue_style_cascade, shared_vue_style_plan, transform_vue_style, CascadeInput,
    CssStageRequest, VueStyleCascadeOutcome, VueStyleFacts,
};
use crate::framework::vue::style::modules::analyze_css_module_classes;
use crate::style_planner::{
    authored_or_parsed_ir, AuthoredStyleInput, PlainCssInput, PreparedStyleIr, StyleRewriteFailure,
    StyleRewriteFailureClass, StyleRewriteStage, VerifiedPlainCss,
};
use crate::work_ledger::{CompilerWorkLedger, WorkKind};

/// The component's scope identity: eight lowercase hex digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeId([u8; 8]);

impl ScopeId {
    /// From the component identity (the same hash the host shows).
    pub fn for_component(component_id: &str) -> Self {
        Self(compute_scope_id(component_id))
    }
    pub(crate) fn for_runtime_component(name: &str, component_id: Option<&str>) -> Self {
        Self(crate::compile::resolve_scope_id(name, component_id))
    }
    /// The eight hex digits.
    pub fn hash(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or_default()
    }
    /// The attribute name, `data-v-<hash>`.
    pub fn attribute(&self) -> String {
        format!("data-v-{}", self.hash())
    }
    /// The slotted-content attribute name, `data-v-<hash>-s`.
    pub fn slotted_attribute(&self) -> String {
        format!("data-v-{}-s", self.hash())
    }
}

/// One `v-bind()` in a style block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VueCssVar {
    pub style_index: u32,
    /// The custom property name, spelled by `generate_var_name`.
    pub var_name: String,
    /// The bound expression's source text.
    pub expression: String,
    /// Where the expression sits in the SFC source.
    pub expression_span: Span,
}

/// One `<style module>` block's exposed classes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VueCssModuleBinding {
    pub style_index: u32,
    /// `$style`, or the name given by `module="name"`.
    pub binding_name: String,
    /// Authored class name to generated class name, in source order.
    pub classes: Vec<(String, String)>,
}

/// The one scope plan of a component. Template, style, server output and
/// metadata all read scope identity from this value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VueComponentScopePlan {
    pub scope: ScopeId,
    /// At least one `<style scoped>` block exists.
    pub has_scoped: bool,
    /// A scoped block uses `:slotted()`.
    pub has_slotted: bool,
    pub css_vars: Vec<VueCssVar>,
    /// `false` when a block could not be read completely; consumers must
    /// then treat every binding as possibly used by a style.
    pub css_vars_complete: bool,
    pub modules: Vec<VueCssModuleBinding>,
}

impl VueComponentScopePlan {
    pub(crate) fn new(scope: ScopeId) -> Self {
        Self {
            scope,
            has_scoped: false,
            has_slotted: false,
            css_vars: Vec::new(),
            css_vars_complete: true,
            modules: Vec::new(),
        }
    }

    /// Read the completed cascade's facts. A caller that already obtained
    /// module classes supplies them too; no style is parsed or rewritten
    /// again to build the component scope.
    pub(crate) fn record_block(
        &mut self,
        block: &VueStyleBlockInput<'_>,
        origin: u32,
        plain: bool,
        facts: &VueStyleFacts,
        failures: &[StyleRewriteFailure],
        classes: Option<&[(String, String)]>,
    ) {
        self.has_scoped |= block.scoped;
        self.has_slotted |= block.scoped && plain && facts.rewrites.slotted;
        self.css_vars
            .extend(facts.v_bind_vars.iter().map(|var| VueCssVar {
                style_index: block.index,
                var_name: var.var_name.clone(),
                expression: var.expression.clone(),
                expression_span: Span::new(origin + var.expr_start, origin + var.expr_end),
            }));
        self.css_vars_complete &= !facts.pulls_in_unparsed_bytes()
            && failures
                .iter()
                .all(|failure| failure.stage != StyleRewriteStage::AuthoredVBind);
        if let Some(name) = block.module {
            let classes = classes.map(|classes| classes.to_vec()).unwrap_or_else(|| {
                if plain {
                    facts.module_classes.clone()
                } else {
                    analyze_css_module_classes(block.authored, self.scope.hash())
                        .unwrap_or_default()
                }
            });
            self.modules.push(VueCssModuleBinding {
                style_index: block.index,
                binding_name: if name.is_empty() {
                    "$style".to_owned()
                } else {
                    name.to_owned()
                },
                classes,
            });
        }
    }
}

/// One style block as the host supplied it.
pub struct VueStyleBlockInput<'a> {
    pub index: u32,
    pub scoped: bool,
    /// `Some("")` for a bare `module` attribute.
    pub module: Option<&'a str>,
    pub authored: AuthoredStyleInput<'a>,
    /// The parsed block when the host retained one.
    pub prepared: Option<&'a PreparedStyleIr>,
    /// The stage `authored` is at.
    pub stage: StyleStage,
}

#[derive(Debug, Clone)]
pub enum VueStyleOutcome {
    /// Rewritten CSS with its decoded map (when maps were requested).
    Rewritten {
        code: String,
        map: Option<DecodedMap>,
        result: Box<QualifiedStyleResult>,
    },
    /// No Vue rewrite applied; the supplied bytes publish unchanged.
    Unchanged,
    /// The block must be supplied at a later stage first.
    NeedInput(InputDemand),
    Refused(StyleRewriteFailure),
}

#[derive(Debug, Clone)]
pub struct VueStyleBlockPlan {
    pub index: u32,
    pub scoped: bool,
    pub module: Option<String>,
    pub outcome: VueStyleOutcome,
}

/// The style result of one component.
#[derive(Debug, Clone)]
pub struct VueStylePlan {
    pub scope: VueComponentScopePlan,
    pub blocks: Vec<VueStyleBlockPlan>,
}

impl VueStylePlan {
    /// Inputs still needed, in block order; empty when the plan is final.
    pub fn needed_inputs(&self) -> Vec<InputDemand> {
        self.blocks
            .iter()
            .filter_map(|block| match &block.outcome {
                VueStyleOutcome::NeedInput(demand) => Some(*demand),
                _ => None,
            })
            .collect()
    }
}

/// Plan every style block of a component. Reads each block's syntax tree;
/// scans no CSS text.
pub fn plan_vue_styles<M: ObserveExec>(
    component_id: &str,
    blocks: &[VueStyleBlockInput<'_>],
    semantic: Option<&VueSemanticSnapshot<'_>>,
    rewrite: bool,
    map_mode: MapMode,
    ledger: &mut CompilerWorkLedger<M>,
) -> VueStylePlan {
    plan_vue_styles_in_scope(
        ScopeId::for_component(component_id),
        blocks,
        semantic,
        rewrite,
        map_mode,
        ledger,
    )
}

/// Plan with the scope identity already resolved by the request owner.
pub(crate) fn plan_vue_styles_in_scope<M: ObserveExec>(
    scope_id: ScopeId,
    blocks: &[VueStyleBlockInput<'_>],
    semantic: Option<&VueSemanticSnapshot<'_>>,
    rewrite: bool,
    map_mode: MapMode,
    ledger: &mut CompilerWorkLedger<M>,
) -> VueStylePlan {
    let hash = scope_id.hash();
    let mut scope = VueComponentScopePlan::new(scope_id);
    let mut planned = Vec::with_capacity(blocks.len());
    for block in blocks {
        ledger.record(WorkKind::StyleBlocks, 1);
        let authored = input_with_retained_parse(block);
        let wants_module = block.module.is_some();
        let plain = PlainCssInput::try_new(
            authored.code,
            authored.dialect,
            authored.source.source_name,
            authored.source.source_space_token,
            authored.source.content_artifact_token,
        )
        .is_ok();
        let scoped_runs = block.scoped && plain;
        if scoped_runs {
            ledger.record(WorkKind::StyleSelectorPlans, 1);
        }
        let needs_preprocessed = rewrite
            && block.stage == StyleStage::Authored
            && !plain
            && (wants_module || block.scoped);
        let mut run = if rewrite && !needs_preprocessed {
            rewrite_block(block, authored, hash, wants_module, block.scoped, map_mode)
        } else {
            plan_block_facts(authored, hash, wants_module, block.scoped)
        };
        if needs_preprocessed {
            run.outcome = VueStyleOutcome::NeedInput(InputDemand::ExternalStyleStage {
                style_index: block.index,
                stage: StyleStage::Preprocessed,
            });
        }

        scope.record_block(
            &VueStyleBlockInput { authored, ..*block },
            block_origin(authored),
            plain,
            &run.facts,
            &run.failures,
            None,
        );
        planned.push(VueStyleBlockPlan {
            index: block.index,
            scoped: block.scoped,
            module: block.module.map(str::to_string),
            outcome: run.outcome,
        });
    }
    VueStylePlan {
        scope,
        blocks: planned,
    }
}

/// What one block contributed: its facts, the stage failures it hit and the
/// outcome published for it.
struct BlockRun {
    facts: VueStyleFacts,
    failures: Vec<StyleRewriteFailure>,
    outcome: VueStyleOutcome,
}

/// The block's authored input, with the host's retained parse attached when
/// the input does not already carry one.
fn input_with_retained_parse<'a>(block: &VueStyleBlockInput<'a>) -> AuthoredStyleInput<'a> {
    match (block.authored.prepared, block.prepared) {
        (None, Some(prepared)) => block.authored.with_prepared(prepared.ir()),
        _ => block.authored,
    }
}

/// Where the block's own bytes start in the SFC: the origin the host gave the
/// retained parse, or zero when the block was not parsed with one. A host that
/// wants SFC-relative expression spans supplies the parse with its origin.
fn block_origin(authored: AuthoredStyleInput<'_>) -> u32 {
    authored.prepared.map_or(0, |ir| ir.source().origin())
}

/// Plan every stage the block requests over one parse, without producing any
/// output bytes.
fn plan_block_facts(
    authored: AuthoredStyleInput<'_>,
    hash: &str,
    module: bool,
    scoped: bool,
) -> BlockRun {
    let css = CssStageRequest::gated(authored, module, scoped);
    match authored_or_parsed_ir(authored, StyleRewriteStage::AuthoredVBind) {
        Ok(parsed) => {
            let plan =
                shared_vue_style_plan(&parsed, authored.dialect, hash, css.module, css.scoped);
            BlockRun {
                facts: plan.facts,
                failures: plan.failures.into_vec(),
                outcome: VueStyleOutcome::Unchanged,
            }
        }
        Err(failure) => BlockRun {
            facts: VueStyleFacts::default(),
            failures: vec![failure.clone()],
            outcome: VueStyleOutcome::Refused(failure),
        },
    }
}

/// Run the cascade over the block and publish what it produced.
fn rewrite_block(
    block: &VueStyleBlockInput<'_>,
    authored: AuthoredStyleInput<'_>,
    hash: &str,
    module: bool,
    scoped: bool,
    map_mode: MapMode,
) -> BlockRun {
    let want_map = map_mode == MapMode::Map;
    let outcome = match block.stage {
        StyleStage::Authored => run_vue_style_cascade(authored, hash, module, scoped, want_map),
        _ => {
            let parsed =
                match authored_or_parsed_ir(authored, StyleRewriteStage::PostPreprocessModules) {
                    Ok(parsed) => parsed,
                    Err(failure) => {
                        return BlockRun {
                            facts: VueStyleFacts::default(),
                            failures: vec![failure.clone()],
                            outcome: VueStyleOutcome::Refused(failure),
                        }
                    }
                };
            let Some(verified) = VerifiedPlainCss::from_parsed_native_css(&parsed) else {
                let failure = StyleRewriteFailure::new(
                    StyleRewriteFailureClass::StageRequiresPlainCss,
                    StyleRewriteStage::PostPreprocessModules,
                    authored.dialect,
                    None,
                );
                return BlockRun {
                    facts: VueStyleFacts::default(),
                    failures: vec![failure.clone()],
                    outcome: VueStyleOutcome::Refused(failure),
                };
            };
            transform_vue_style(
                verified,
                CascadeInput::Preprocessed(PreprocessorIdentity::Anonymous),
                authored.source.source_name,
                authored.source.source_space_token,
                authored.source.content_artifact_token,
                hash,
                module,
                scoped,
                want_map,
            )
        }
    };
    published_outcome(outcome, authored.code, want_map)
}

/// Map a finished cascade run to the outcome published for its block.
fn published_outcome(
    outcome: VueStyleCascadeOutcome,
    authored_code: &str,
    want_map: bool,
) -> BlockRun {
    let VueStyleCascadeOutcome {
        result,
        source_map,
        facts,
        stage_failures,
    } = outcome;
    let published = if let Some(first) = stage_failures.first() {
        VueStyleOutcome::Refused(first.clone())
    } else if !facts.rewrites.any_applied() && result.code() == authored_code {
        VueStyleOutcome::Unchanged
    } else {
        let code = result.code().to_string();
        let map = if want_map {
            decode_style_map(&source_map, &code, authored_code)
        } else {
            None
        };
        VueStyleOutcome::Rewritten {
            code,
            map,
            result: Box::new(result),
        }
    };
    BlockRun {
        facts,
        failures: stage_failures,
        outcome: published,
    }
}

/// Decode the cascade's map to byte segments. The map is validated by the
/// assembly decoder and only then read; a map that does not validate is
/// published as no map.
fn decode_style_map(raw: &str, generated: &str, source: &str) -> Option<DecodedMap> {
    let decoded = validate_and_decode(raw, generated).ok()?;
    let generated_starts = generated_line_starts(generated);
    let source_starts = generated_line_starts(source);
    let mut anchors: Vec<(u32, u32, u32)> = decoded
        .segments()
        .iter()
        .filter_map(|wire| {
            let payload = wire.payload?;
            let start = utf16_offset_to_bytes(
                generated,
                &generated_starts,
                wire.generated_line,
                wire.generated_column,
            )?;
            let source_start = utf16_offset_to_bytes(
                source,
                &source_starts,
                payload.source_line,
                payload.source_column,
            )?;
            Some((start, payload.source_index, source_start))
        })
        .collect();
    anchors.sort_by_key(|anchor| anchor.0);
    anchors.dedup_by_key(|anchor| anchor.0);
    // A token answers its source point until the next token starts.
    let generated_end = u32::try_from(generated.len()).ok()?;
    let segments: Vec<DecodedMapSegment> = anchors
        .iter()
        .enumerate()
        .filter_map(|(index, &(start, source_index, source_start))| {
            let end = anchors.get(index + 1).map_or(generated_end, |next| next.0);
            (end > start).then_some(DecodedMapSegment {
                generated: start..end,
                source: source_index,
                source_start,
                kind: MapSegmentKind::Anchor,
            })
        })
        .collect();
    Some(DecodedMap::from_decoded_fragment(&decoded, segments))
}
