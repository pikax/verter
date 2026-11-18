//! One component's Vue semantic facts, produced once per demanded family.
//!
//! [`build_semantic_snapshot`] reads the demand's fact families and runs only
//! their producers: the binding table from the bindings the script frontend
//! reported, the origin table from the component's script origin view, the
//! macro facts as supplied, and the template family from the one parse of
//! every template expression. Everything else reads the snapshot.

use std::cell::OnceCell;

use oxc_allocator::Allocator;
use oxc_span::SourceType;
use rustc_hash::FxHashMap;
use verter_audit::observe::ObserveExec;
use verter_semantic::compiler_view::ScriptOriginView;
use verter_session_query::analysis::script_snapshot::ScriptAnalysisSnapshot;

use super::binding::VueBindingTable;
use super::origin::{build_origin_table, FrameworkOriginTable};
use crate::compile::template_expr_overlay::component_tag_candidates;
use crate::compile::types::VueMacroSemanticInput;
use crate::compile_request::demand::{
    Coverage, DemandSet, FactFamilies, FactFamily, SemanticAdmission,
};
use crate::template::oxc::parse_template_expressions;
use crate::template::oxc::scope::LexicalScopeId;
use crate::template::oxc::types::{
    ComponentSlotSummary, OxcNodeData, OxcParsedAst, OxcParsedElement, OxcParsedExpression,
};
use crate::template::topology::{
    RegionCompleteness, VueTemplateNodeId, VueTemplateNodeKind, VueTemplateTopology,
};
use crate::template::vue_syntax::{builtin_component, camelize, to_pascal_case, BuiltinComponent};
use crate::types::BindingType;
use crate::work_ledger::{CompilerWorkLedger, WorkKind};

/// What a component tag refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VueComponentClass {
    /// Not a component.
    None,
    /// A script binding (import or local).
    Binding,
    /// A framework built-in (`KeepAlive`, `Teleport`, …).
    BuiltIn,
    /// `<component :is>`.
    Dynamic,
    /// The component's own file name.
    SelfReference,
    /// Resolved by name at run time.
    Resolved,
}

/// The script binding a component tag resolved to: the binding's own name
/// and what the tag appends to it (`.Item` of `<Swiper.Item>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VueComponentBindingMatch {
    pub name: String,
    pub member_tail: String,
}

/// What the snapshot knows of one component tag: its class, and the fact the
/// class names (the built-in, the matched binding).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VueComponentTagFacts {
    pub class: VueComponentClass,
    pub built_in: Option<BuiltinComponent>,
    pub binding: Option<VueComponentBindingMatch>,
}

/// The facts of a node that is not a component.
pub static NO_COMPONENT_TAG: VueComponentTagFacts =
    VueComponentTagFacts::of_class(VueComponentClass::None);

impl VueComponentTagFacts {
    const fn of_class(class: VueComponentClass) -> Self {
        Self {
            class,
            built_in: None,
            binding: None,
        }
    }
}

/// Everything the authority reads. All borrowed for one compile.
pub struct VueSemanticInputs<'c> {
    pub source: &'c str,
    pub topology: Option<&'c VueTemplateTopology<'c>>,
    /// Bindings the script frontend reported.
    pub reported_bindings: FxHashMap<&'c str, BindingType>,
    /// Script analysis of the `<script setup>` block: with the companion's,
    /// the only input of the component's script origin view.
    pub setup_analysis: Option<&'c ScriptAnalysisSnapshot>,
    /// Script analysis of the companion `<script>` block.
    pub companion_analysis: Option<&'c ScriptAnalysisSnapshot>,
    pub macros: &'c VueMacroSemanticInput,
    /// Framework module specifier (`vue` by default).
    pub runtime_module: &'c str,
    /// PascalCase name of the component's own file.
    pub self_name: &'c str,
    pub template_is_ts: bool,
    pub allocator: &'c Allocator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VueSemanticRefusal {
    /// A demanded family needs an input that was not supplied.
    MissingInput { family: FactFamily },
    /// The template topology is not complete enough for compilation.
    IncompleteTopology,
}

/// The template family: the one parse of every template expression and the
/// class of every component tag.
struct TemplateFacts<'c> {
    topology: &'c VueTemplateTopology<'c>,
    source: &'c str,
    expressions: TemplateExpressions<'c>,
    /// Indexed by topology node.
    component_tags: Vec<VueComponentTagFacts>,
}

enum TemplateExpressions<'c> {
    Parsed(OxcParsedAst<'c>),
    Retained(&'c OxcParsedAst<'c>),
}

impl<'c> std::ops::Deref for TemplateExpressions<'c> {
    type Target = OxcParsedAst<'c>;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Parsed(expressions) => expressions,
            Self::Retained(expressions) => expressions,
        }
    }
}

/// The facts of one component. A family that was not demanded is absent:
/// its accessor fails a debug assertion, returns an empty value, and the
/// family performed no work.
pub struct VueSemanticSnapshot<'c> {
    families: FactFamilies,
    admission: SemanticAdmission,
    bindings: VueBindingTable<'c>,
    origins: Option<FrameworkOriginTable>,
    template: Option<TemplateFacts<'c>>,
    macros: &'c VueMacroSemanticInput,
    /// The empty values an undemanded accessor returns, built only then.
    empty_origins: OnceCell<FrameworkOriginTable>,
    empty_expressions: OnceCell<OxcParsedAst<'c>>,
}

impl<'c> VueSemanticSnapshot<'c> {
    /// The families this snapshot produced.
    pub fn families(&self) -> FactFamilies {
        self.families
    }

    #[allow(
        dead_code,
        reason = "the compile admission reads it once a target composes the snapshot"
    )]
    pub(crate) fn admission(&self) -> &SemanticAdmission {
        &self.admission
    }

    fn produced(&self, family: FactFamily) -> bool {
        let produced = self.families.contains(family);
        verter_debug_assert!(produced, "the {family:?} family was not demanded");
        produced
    }

    /// Family: bindings.
    pub fn bindings(&self) -> &VueBindingTable<'c> {
        self.produced(FactFamily::Bindings);
        &self.bindings
    }

    /// Family: imports.
    pub fn origins(&self) -> &FrameworkOriginTable {
        self.produced(FactFamily::Imports);
        self.origins.as_ref().unwrap_or_else(|| {
            self.empty_origins
                .get_or_init(FrameworkOriginTable::default)
        })
    }

    /// Family: macros.
    pub fn macros(&self) -> &'c VueMacroSemanticInput {
        static UNAVAILABLE: VueMacroSemanticInput = VueMacroSemanticInput::Unavailable;
        if self.produced(FactFamily::Macros) {
            self.macros
        } else {
            &UNAVAILABLE
        }
    }

    fn template(&self) -> Option<&TemplateFacts<'c>> {
        self.produced(FactFamily::Template);
        self.template.as_ref()
    }

    /// Family: template. The one parsed representation of every template
    /// expression.
    pub fn expressions(&self) -> &OxcParsedAst<'c> {
        match self.template() {
            Some(template) => &template.expressions,
            None => self
                .empty_expressions
                .get_or_init(|| OxcParsedAst::new(Vec::new())),
        }
    }

    /// Parsed directives of an element-like node.
    pub fn element_exprs(&self, node: VueTemplateNodeId) -> Option<&OxcParsedElement<'c>> {
        match self.node_data(node)? {
            OxcNodeData::Element(element) => Some(element),
            OxcNodeData::Interpolation(_) | OxcNodeData::None => None,
        }
    }

    /// Parsed expression of an interpolation node.
    pub fn interpolation(&self, node: VueTemplateNodeId) -> Option<&OxcParsedExpression<'c>> {
        match self.node_data(node)? {
            OxcNodeData::Interpolation(expression) => Some(expression),
            OxcNodeData::Element(_) | OxcNodeData::None => None,
        }
    }

    fn node_data(&self, node: VueTemplateNodeId) -> Option<&OxcNodeData<'c>> {
        let template = self.template()?;
        let syntax = template.topology.syntax(node)?;
        template.expressions.data.get(syntax.0)
    }

    /// Lexical scope the node's own expressions are evaluated in.
    pub fn scope_of(&self, node: VueTemplateNodeId) -> LexicalScopeId {
        let Some(template) = self.template() else {
            return LexicalScopeId::ROOT;
        };
        match template.topology.syntax(node) {
            Some(syntax) => template
                .expressions
                .scope_of(syntax, template.topology.ast()),
            None => LexicalScopeId::ROOT,
        }
    }

    /// Names a lexical scope declares itself.
    pub fn scope_names(&self, scope: LexicalScopeId) -> &[&'c str] {
        match self.template() {
            Some(template) => template.expressions.scopes.own_names(scope),
            None => &[],
        }
    }

    pub fn component_class(&self, node: VueTemplateNodeId) -> VueComponentClass {
        self.component_tag(node).class
    }

    /// The class of a component tag with the fact it names.
    pub fn component_tag(&self, node: VueTemplateNodeId) -> &VueComponentTagFacts {
        self.template()
            .and_then(|template| template.component_tags.get(node.index()))
            .unwrap_or(&NO_COMPONENT_TAG)
    }

    /// Slot grouping of a component's children.
    pub fn slot_summary(&self, component: VueTemplateNodeId) -> Option<&ComponentSlotSummary> {
        let template = self.template()?;
        let syntax = template.topology.syntax(component)?;
        template
            .expressions
            .slot_summary(syntax, template.topology.ast(), template.source)
    }
}

/// Produce the demanded families, each once. Parses every template
/// expression at most once, only when the template family is demanded.
pub fn build_semantic_snapshot<'c, M: ObserveExec>(
    inputs: VueSemanticInputs<'c>,
    demand: &DemandSet,
    ledger: &mut CompilerWorkLedger<M>,
) -> Result<VueSemanticSnapshot<'c>, VueSemanticRefusal> {
    build_semantic_snapshot_with_expressions(inputs, demand, None, ledger)
}

/// Reuse the runtime overlay when the caller has already parsed it.
pub(crate) fn build_semantic_snapshot_with_expressions<'c, M: ObserveExec>(
    inputs: VueSemanticInputs<'c>,
    demand: &DemandSet,
    retained: Option<&'c OxcParsedAst<'c>>,
    ledger: &mut CompilerWorkLedger<M>,
) -> Result<VueSemanticSnapshot<'c>, VueSemanticRefusal> {
    let demanded = demand.fact_families();
    ledger.record(
        WorkKind::SemanticFactFamiliesDemanded,
        u64::from(demanded.count()),
    );

    // Refuse before any producer runs.
    let topology = if demanded.contains(FactFamily::Template) {
        let topology = inputs.topology.ok_or(VueSemanticRefusal::MissingInput {
            family: FactFamily::Template,
        })?;
        if topology.completeness() != RegionCompleteness::Complete {
            return Err(VueSemanticRefusal::IncompleteTopology);
        }
        Some(topology)
    } else {
        None
    };

    let mut produced = FactFamilies::empty();

    let origins = demanded.contains(FactFamily::Imports).then(|| {
        produced = produced.with(FactFamily::Imports);
        build_origins(
            inputs.setup_analysis,
            inputs.companion_analysis,
            inputs.runtime_module,
        )
    });

    let mut bindings = VueBindingTable::default();
    if demanded.contains(FactFamily::Bindings) {
        produced = produced.with(FactFamily::Bindings);
        bindings = VueBindingTable::from_reported(inputs.reported_bindings);
        if let Some(origins) = &origins {
            bindings.record_callee_origins(origins);
        }
    }

    // Reactivity is the binding class with its callee provenance: served by
    // the binding and origin tables, so it is produced exactly when both are.
    if demanded.contains(FactFamily::Reactivity)
        && produced.contains(FactFamily::Bindings)
        && produced.contains(FactFamily::Imports)
    {
        produced = produced.with(FactFamily::Reactivity);
    }

    if demanded.contains(FactFamily::Macros) {
        produced = produced.with(FactFamily::Macros);
    }

    let template = topology.map(|topology| {
        produced = produced.with(FactFamily::Template);
        build_template_facts(
            topology,
            inputs.source,
            &bindings,
            inputs.self_name,
            inputs.allocator,
            retained,
            ledger,
        )
    });

    // Dependency reads are the authored reference records retained by the
    // template parse. The fine-grained executor consumes those records and
    // owns their effect graph; there is no second semantic producer.
    if demanded.contains(FactFamily::Dependencies) && template.is_some() {
        produced = produced.with(FactFamily::Dependencies);
    }

    ledger.record(WorkKind::SemanticFactsProduced, u64::from(produced.count()));
    let coverage = if produced == demanded {
        Coverage::Complete
    } else {
        Coverage::Approximate
    };
    Ok(VueSemanticSnapshot {
        families: produced,
        admission: SemanticAdmission::issued(produced, coverage),
        bindings,
        origins,
        template,
        macros: inputs.macros,
        empty_origins: OnceCell::new(),
        empty_expressions: OnceCell::new(),
    })
}

fn build_origins(
    setup: Option<&ScriptAnalysisSnapshot>,
    companion: Option<&ScriptAnalysisSnapshot>,
    runtime_module: &str,
) -> FrameworkOriginTable {
    #[cfg(test)]
    ORIGIN_VIEW_BUILDS.with(|count| count.set(count.get() + 1));
    build_origin_table(ScriptOriginView::build(setup, companion), runtime_module)
}

fn build_template_facts<'c, M: ObserveExec>(
    topology: &'c VueTemplateTopology<'c>,
    source: &'c str,
    bindings: &VueBindingTable<'c>,
    self_name: &str,
    allocator: &'c Allocator,
    retained: Option<&'c OxcParsedAst<'c>>,
    ledger: &mut CompilerWorkLedger<M>,
) -> TemplateFacts<'c> {
    // Every runtime lane parses template expressions as TSX: a superset that
    // also accepts the JavaScript a non-TS component writes.
    let expressions = match retained {
        Some(expressions) => TemplateExpressions::Retained(expressions),
        None => {
            let expressions = parse_template_expressions(
                topology.ast(),
                source,
                allocator,
                SourceType::tsx(),
                false,
            );
            ledger.record(WorkKind::ParseAuthoritativeExpressionParses, 1);
            if M::CAPTURED {
                ledger.record(
                    WorkKind::ParseExpressionAttempts,
                    expressions.iter_expressions().count() as u64,
                );
            }
            TemplateExpressions::Parsed(expressions)
        }
    };

    let component_tags = topology
        .nodes()
        .map(|node| classify_component(topology, node, source, bindings, self_name))
        .collect();

    TemplateFacts {
        topology,
        source,
        expressions,
        component_tags,
    }
}

/// What a component tag refers to, in the order Vue's `resolveComponentType`
/// decides it: `<component>` is dynamic; a framework built-in comes next; then
/// a script binding under any casing Vue resolves a tag against; then the
/// component's own name; anything else resolves by name at run time.
fn classify_component(
    topology: &VueTemplateTopology<'_>,
    node: VueTemplateNodeId,
    source: &str,
    bindings: &VueBindingTable<'_>,
    self_name: &str,
) -> VueComponentTagFacts {
    if topology.kind(node) != VueTemplateNodeKind::Component {
        return VueComponentTagFacts::of_class(VueComponentClass::None);
    }
    let Some(element) = topology.element(node) else {
        return VueComponentTagFacts::of_class(VueComponentClass::None);
    };
    let tag = &source[(element.tag_open.start + 1) as usize..element.tag_open.name_end as usize];
    if tag == "component" || tag == "Component" {
        return VueComponentTagFacts::of_class(VueComponentClass::Dynamic);
    }
    let pascal = to_pascal_case(tag);
    if let Some(built_in) = builtin_component(tag).or_else(|| builtin_component(&pascal)) {
        return VueComponentTagFacts {
            class: VueComponentClass::BuiltIn,
            built_in: Some(built_in),
            binding: None,
        };
    }
    // A member tag resolves against its root segment; a binding named like
    // the whole tag takes no tail.
    let root_len = tag.find('.').unwrap_or(tag.len());
    let root = &tag[..root_len];
    let camel = camelize(root);
    let pascal_root = to_pascal_case(root);
    // The candidate search also lists names that only suppress an unused
    // diagnostic (`foo` for `<Foo>`); a runtime reference takes only the
    // forms Vue resolves a tag against.
    let resolves_at_run_time = |candidate: &str| {
        candidate == tag || candidate == root || candidate == camel || candidate == pascal_root
    };
    if let Some(name) = component_tag_candidates(tag)
        .into_iter()
        .find(|candidate| resolves_at_run_time(candidate) && bindings.contains(candidate))
    {
        let member_tail = if name == tag {
            String::new()
        } else {
            tag[root_len..].to_string()
        };
        return VueComponentTagFacts {
            class: VueComponentClass::Binding,
            built_in: None,
            binding: Some(VueComponentBindingMatch { name, member_tail }),
        };
    }
    if !self_name.is_empty() && pascal == self_name {
        return VueComponentTagFacts::of_class(VueComponentClass::SelfReference);
    }
    VueComponentTagFacts::of_class(VueComponentClass::Resolved)
}

#[cfg(test)]
thread_local! {
    /// Counts origin views built on the current thread, so a test can prove
    /// an undemanded imports family builds none.
    static ORIGIN_VIEW_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(super) fn origin_view_build_count() -> usize {
    ORIGIN_VIEW_BUILDS.with(std::cell::Cell::get)
}

#[cfg(test)]
#[path = "snapshot_tests.rs"]
mod snapshot_tests;
