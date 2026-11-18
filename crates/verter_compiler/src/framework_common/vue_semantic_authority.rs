//! Vue [`FrameworkSemanticAuthority`] adapter.
//!
//! Owns Vue eval-source and template-fact interpretation over an
//! already-admitted parse artifact. Catalog lookup keys adapter × epoch
//! × Semantic.

use std::sync::Arc;

use verter_audit::observe::ObserveExec;
use verter_language::{FrameworkAdapterId, LanguageId};

use verter_audit::observe::Uncaptured;

use crate::compile::template_data::extract_raw_template_data;
use crate::compile::types::VueMacroSemanticInput;
use crate::compile::{
    collect_expression_errors, convert_diagnostics, template_is_non_html, RawTemplateData,
};
use crate::compile_request::demand::{close_demand, DemandSeed, DemandSet, TargetKind};
use crate::compile_request::{ProductKind, RuntimeStyleProcessing};
use crate::framework::vue::semantic::{
    build_semantic_snapshot, VueSemanticInputs, VueSemanticRefusal, VueSemanticSnapshot,
};
use crate::script::bindings::script_lane_bindings;
use crate::script::prepared::PreparedScript;
use crate::template::topology::{build_template_topology, TopologyRecoveryFacts};
use crate::work_ledger::CompilerWorkLedger;

use super::capability::{FrameworkSemanticAuthority, Present};
use super::carrier_compiler::RuntimeDiagnostic;
use super::catalog::{SemanticCap, TypedCapabilityRegistration};
use super::registered_carrier_projection::TemplateFactsProduct;
use super::vue_carrier_frontend::{VueCarrierFrontend, VueSfcV3};
use super::FrameworkParseArtifact;

/// Position-preserving eval source over admitted script regions.
///
/// Script bytes stay at their raw offsets; every other byte is blanked
/// (line terminators preserved). Output length equals input length. It
/// does not inject a newline between adjacent script regions.
pub(crate) fn position_preserving_eval_source(
    source: &str,
    artifact: &FrameworkParseArtifact,
) -> Arc<str> {
    let src = source.as_bytes();
    let mut out: Vec<u8> = src
        .iter()
        .map(|&b| if b == b'\n' || b == b'\r' { b } else { b' ' })
        .collect();
    for region in artifact.script_regions() {
        let start = region.span.start as usize;
        let end = region.span.end as usize;
        if start <= end && end <= src.len() {
            out[start..end].copy_from_slice(&src[start..end]);
        }
    }
    match String::from_utf8(out) {
        Ok(text) => Arc::from(text),
        Err(_) => Arc::from(source),
    }
}

/// Vue semantic authority: eval-source, template facts, typed identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VueSemanticAuthority;

/// Admission token issued only over an already-admitted Vue parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VueSemanticAdmission {
    _private: (),
}

impl VueSemanticAuthority {
    /// Issue the semantic admission over an already-admitted Vue parse:
    /// the parse admission's witnessed [`verter_language::ParseKey`] must
    /// be the artifact's own, and the artifact's epoch must select this
    /// authority's registered semantic catalog row. Crate-private —
    /// reached only from host-integration composition, and only with the
    /// parse admission in hand; product backends never mint it.
    pub(crate) fn admit_over_parse(
        &self,
        parse: &super::vue_carrier_frontend::VueParseAdmission,
        artifact: &FrameworkParseArtifact,
    ) -> Option<VueSemanticAdmission> {
        if parse.parse_key().as_ref() != artifact.parse_key() {
            return None;
        }
        super::registered_carrier_projection::registered_semantic_for(
            artifact.carrier_profile(),
            artifact.framework_profile(),
        )
        .map(|_| VueSemanticAdmission { _private: () })
    }

    /// Serve one component's semantic snapshot: the fact families `demand`
    /// names, each produced once, with the semantic admission over them.
    /// `admission` witnesses that the component's parse was admitted; the
    /// facts are read only from `inputs`, which hold that parse's products.
    pub fn snapshot<'c, M: ObserveExec>(
        &self,
        admission: &VueSemanticAdmission,
        inputs: VueSemanticInputs<'c>,
        demand: &DemandSet,
        ledger: &mut CompilerWorkLedger<M>,
    ) -> Result<VueSemanticSnapshot<'c>, VueSemanticRefusal> {
        let VueSemanticAdmission { _private: () } = admission;
        build_semantic_snapshot(inputs, demand, ledger)
    }

    /// Adapter this authority answers to.
    #[must_use]
    pub fn adapter_id(&self) -> FrameworkAdapterId {
        VueCarrierFrontend.adapter_id()
    }

    /// Carrier language this authority interprets.
    #[must_use]
    pub fn carrier_language_id(&self) -> LanguageId {
        VueCarrierFrontend.carrier_language_id()
    }
}

impl FrameworkSemanticAuthority<VueSfcV3> for VueSemanticAuthority {
    type EvalSource = Arc<str>;
    type TemplateFacts = Option<TemplateFactsProduct>;
    type StyleMeaning = ();
    type SemanticAdmission = VueSemanticAdmission;
    type ParseArtifact = FrameworkParseArtifact;

    fn eval_source(&self, source: &str, artifact: &FrameworkParseArtifact) -> Arc<str> {
        position_preserving_eval_source(source, artifact)
    }

    fn template_facts(
        &self,
        source: &str,
        artifact: &FrameworkParseArtifact,
    ) -> Option<TemplateFactsProduct> {
        let parsed = VueCarrierFrontend.parsed_sfc(artifact)?;
        let Some(template_ast) = parsed.template_ast() else {
            return Some(TemplateFactsProduct {
                data: RawTemplateData::default(),
                diagnostics: Vec::new(),
            });
        };
        // A template the parser recovered, or one in a non-HTML language,
        // has no facts to stand on.
        if parsed.has_errors() || template_is_non_html(template_ast, source) {
            return None;
        }

        let allocator = oxc_allocator::Allocator::new();
        let mut ledger = CompilerWorkLedger::<Uncaptured>::new();
        // The script lane's bindings: these facts are read with no macro
        // semantics, so a type-based macro binds no prop names here.
        let prepared =
            PreparedScript::build(source, parsed.script(), parsed.script_setup(), &allocator);
        let lane = script_lane_bindings(parsed.script_setup(), &prepared, None, &allocator);
        let topology = build_template_topology(
            source,
            template_ast,
            TopologyRecoveryFacts::default(),
            &mut ledger,
        )
        .ok()?;
        let macros = VueMacroSemanticInput::Unavailable;
        let snapshot = build_semantic_snapshot(
            VueSemanticInputs {
                source,
                topology: Some(&topology),
                reported_bindings: lane.bindings.clone(),
                setup_analysis: None,
                companion_analysis: None,
                macros: &macros,
                runtime_module: "vue",
                self_name: "",
                template_is_ts: false,
                allocator: &allocator,
            },
            &template_fact_demand(parsed.script().is_some() || parsed.script_setup().is_some()),
            &mut ledger,
        )
        .ok()?;
        let expressions = snapshot.expressions();

        // The template expressions are parsed only here on this route, so
        // their diagnostics (e.g. `XInvalidExpression` for a malformed `v-if`
        // expression) travel WITH the facts: a consumer that drops them
        // erases the file's template expression errors.
        let mut expression_diagnostics = Vec::new();
        let mut compile_failures = Vec::new();
        collect_expression_errors(
            expressions,
            source,
            &mut expression_diagnostics,
            &mut compile_failures,
        );
        let mut extraction_diagnostics = convert_diagnostics(&expression_diagnostics);
        extraction_diagnostics.extend(compile_failures);
        let diagnostics: Vec<RuntimeDiagnostic> = extraction_diagnostics
            .into_iter()
            .map(|d| RuntimeDiagnostic {
                severity: d.severity.into(),
                code: d.code,
                message: d.message,
                // A diagnostic with no mapped construct location is a
                // whole-component result at this boundary.
                span: d
                    .span
                    .unwrap_or_else(|| verter_span::Span::new(0, source.len() as u32)),
            })
            .collect();
        Some(TemplateFactsProduct {
            data: extract_raw_template_data(template_ast, expressions, source, &lane.bindings),
            diagnostics,
        })
    }
}

/// The demand of a component's template facts: its bindings and its
/// template family. A server-shaped seed closes to exactly those facts (with
/// macros, which cost nothing to serve), and to no reactivity, imports or
/// dependency family.
fn template_fact_demand(has_script: bool) -> DemandSet {
    close_demand(&[DemandSeed {
        product: ProductKind::Analysis,
        target: TargetKind::Server,
        runtime_source_map: false,
        style_processing: RuntimeStyleProcessing::AuthoredOnly,
        has_template: true,
        has_script,
        style_count: 0,
    }])
}

/// Typed Vue semantic catalog row.
#[must_use]
pub fn vue_semantic_authority_registration(
) -> TypedCapabilityRegistration<SemanticCap<VueSemanticAuthority>> {
    TypedCapabilityRegistration::register_semantic::<VueSfcV3, _>(
        VueSemanticAuthority.adapter_id(),
        VueSemanticAuthority.carrier_language_id(),
        Present(VueSemanticAuthority),
    )
    .expect("the built-in Vue profile manifest is admitted")
}

#[cfg(test)]
#[path = "vue_semantic_authority_tests.rs"]
mod tests;
