use oxc_allocator::Allocator;
use oxc_span::SourceType;
use verter_audit::observe::Uncaptured;
use verter_semantic::analysis::{build_script_analysis_with_scope, AnalysisScope};

use super::*;
use crate::ast::types::TemplateAst;
use crate::compile_request::demand::{close_demand, DemandSeed, TargetKind};
use crate::compile_request::{ProductKind, RuntimeStyleProcessing};
use crate::framework::vue::semantic::{FrameworkApi, FrameworkOrigin, OriginEvidence};
use crate::template::oxc::{
    parse_template_expressions_call_count, reset_parse_template_expressions_calls,
};
use crate::template::topology::{build_template_topology, TopologyRecoveryFacts};

/// A component's source with its parsed template.
struct Component {
    source: String,
    ast: Option<TemplateAst>,
}

impl Component {
    fn new(template: &str) -> Self {
        let source = format!("<template>{template}</template>");
        let ast = crate::framework_common::vue_bridge::parse_sfc(&source, None, None).template_ast;
        Self { source, ast }
    }

    fn topology(&self) -> VueTemplateTopology<'_> {
        build_template_topology(
            &self.source,
            self.ast.as_ref().expect("template block"),
            TopologyRecoveryFacts::default(),
            &mut CompilerWorkLedger::<Uncaptured>::new(),
        )
        .expect("topology builds")
    }

    /// The topology node whose span starts at the first `text`.
    fn node(&self, topology: &VueTemplateTopology<'_>, text: &str) -> VueTemplateNodeId {
        let start = self.source.find(text).expect("text in source") as u32;
        topology
            .nodes()
            .find(|&node| {
                topology.kind(node) != VueTemplateNodeKind::Root
                    && topology.span(node).start == start
            })
            .unwrap_or_else(|| panic!("no node starts at {text:?}"))
    }
}

fn seed(product: ProductKind, target: TargetKind, has_template: bool) -> DemandSeed {
    DemandSeed {
        product,
        target,
        runtime_source_map: false,
        style_processing: RuntimeStyleProcessing::Complete,
        has_template,
        has_script: true,
        style_count: 0,
    }
}

fn client_tree() -> DemandSeed {
    seed(ProductKind::RuntimeClient, TargetKind::ClientTree, true)
}

fn server() -> DemandSeed {
    seed(ProductKind::RuntimeServer, TargetKind::Server, true)
}

fn inputs<'c>(
    source: &'c str,
    topology: Option<&'c VueTemplateTopology<'c>>,
    reported: &[(&'c str, BindingType)],
    allocator: &'c Allocator,
) -> VueSemanticInputs<'c> {
    VueSemanticInputs {
        source,
        topology,
        reported_bindings: reported.iter().copied().collect(),
        setup_analysis: None,
        companion_analysis: None,
        macros: &VueMacroSemanticInput::Unavailable,
        runtime_module: "vue",
        self_name: "App",
        template_is_ts: false,
        allocator,
    }
}

fn build<'c>(
    inputs: VueSemanticInputs<'c>,
    seeds: &[DemandSeed],
) -> Result<VueSemanticSnapshot<'c>, VueSemanticRefusal> {
    build_semantic_snapshot(
        inputs,
        &close_demand(seeds),
        &mut CompilerWorkLedger::<Uncaptured>::new(),
    )
}

#[test]
fn a_retained_runtime_overlay_is_not_parsed_again() {
    let component = Component::new("<p>{{ count }}</p>");
    let topology = component.topology();
    let allocator = Allocator::default();
    let expressions = parse_template_expressions(
        topology.ast(),
        &component.source,
        &allocator,
        SourceType::tsx(),
        false,
    );
    reset_parse_template_expressions_calls();
    let snapshot = build_semantic_snapshot_with_expressions(
        inputs(
            &component.source,
            Some(&topology),
            &[("count", BindingType::SetupRef)],
            &allocator,
        ),
        &close_demand(&[client_tree()]),
        Some(&expressions),
        &mut CompilerWorkLedger::<Uncaptured>::new(),
    )
    .unwrap();
    assert_eq!(parse_template_expressions_call_count(), 0);
    assert!(std::ptr::eq(snapshot.expressions(), &expressions));
    assert_eq!(snapshot.expressions().iter_expressions().count(), 1);
}

#[test]
fn undemanded_template_family_parses_no_expression() {
    let component = Component::new("<p>{{ a }}</p>");
    let topology = component.topology();
    let allocator = Allocator::default();
    reset_parse_template_expressions_calls();

    let template_less = seed(ProductKind::RuntimeClient, TargetKind::ClientTree, false);
    let snapshot = build(
        inputs(&component.source, Some(&topology), &[], &allocator),
        &[template_less],
    )
    .expect("snapshot");

    assert_eq!(parse_template_expressions_call_count(), 0);
    assert!(!snapshot.families().contains(FactFamily::Template));
    assert!(snapshot.families().contains(FactFamily::Bindings));
}

#[test]
fn template_family_parses_once() {
    let component = Component::new(r#"<p :title="t">{{ a }}</p>"#);
    let topology = component.topology();
    let allocator = Allocator::default();
    reset_parse_template_expressions_calls();

    // Two targets planned, one snapshot: one parse.
    let snapshot = build(
        inputs(&component.source, Some(&topology), &[], &allocator),
        &[client_tree(), server()],
    )
    .expect("snapshot");

    assert_eq!(parse_template_expressions_call_count(), 1);
    let paragraph = component.node(&topology, "<p");
    let interpolation = component.node(&topology, "{{");
    assert!(snapshot.element_exprs(paragraph).is_some());
    let expression = snapshot.interpolation(interpolation).expect("parsed");
    assert!(expression.bindings.is_some());
    assert_eq!(snapshot.scope_of(interpolation), LexicalScopeId::ROOT);
}

#[test]
fn fine_grained_dependencies_reuse_the_template_reference_records() {
    use crate::compile_request::demand::{CompileAdmission, CompileProgress, ParseAdmission};

    let component = Component::new("<p>{{ value + value }}</p>");
    let topology = component.topology();
    let allocator = Allocator::default();
    let demand = close_demand(&[seed(
        ProductKind::RuntimeClient,
        TargetKind::ClientFineGrained,
        true,
    )]);
    reset_parse_template_expressions_calls();
    let snapshot = build_semantic_snapshot(
        inputs(&component.source, Some(&topology), &[], &allocator),
        &demand,
        &mut CompilerWorkLedger::<Uncaptured>::new(),
    )
    .expect("snapshot");

    assert_eq!(snapshot.families(), demand.fact_families());
    assert!(matches!(
        CompileAdmission::compose(
            &demand,
            &ParseAdmission::complete(),
            snapshot.admission(),
            Vec::new()
        ),
        Ok(CompileProgress::Admitted)
    ));
    let interpolation = component.node(&topology, "{{");
    let references = &snapshot
        .interpolation(interpolation)
        .unwrap()
        .bindings
        .as_ref()
        .unwrap()
        .bindings;
    assert_eq!(references.len(), 2);
    assert_eq!(
        references[0].pos as usize,
        component.source.find("value").unwrap()
    );
    assert_eq!(
        references[1].pos as usize,
        component.source.rfind("value").unwrap()
    );
    assert_eq!(parse_template_expressions_call_count(), 1);
}

#[test]
fn template_scopes_come_from_the_one_parse() {
    let component = Component::new(r#"<li v-for="item in items">{{ item }}</li>"#);
    let topology = component.topology();
    let allocator = Allocator::default();
    let snapshot = build(
        inputs(&component.source, Some(&topology), &[], &allocator),
        &[client_tree()],
    )
    .expect("snapshot");

    let interpolation = component.node(&topology, "{{");
    let scope = snapshot.scope_of(interpolation);
    assert_ne!(scope, LexicalScopeId::ROOT);
    assert_eq!(snapshot.scope_names(scope), ["item"]);
}

#[test]
fn component_classes() {
    let component = Component::new(
        r#"<KeepAlive><component :is="view" /></KeepAlive><Child /><App /><unknown-el /><div />"#,
    );
    let topology = component.topology();
    let allocator = Allocator::default();
    let snapshot = build(
        inputs(
            &component.source,
            Some(&topology),
            &[
                ("Child", BindingType::SetupConst),
                ("view", BindingType::SetupRef),
            ],
            &allocator,
        ),
        &[client_tree()],
    )
    .expect("snapshot");

    let class = |text: &str| snapshot.component_class(component.node(&topology, text));
    assert_eq!(class("<KeepAlive"), VueComponentClass::BuiltIn);
    assert_eq!(class("<component"), VueComponentClass::Dynamic);
    assert_eq!(class("<Child"), VueComponentClass::Binding);
    assert_eq!(class("<App"), VueComponentClass::SelfReference);
    assert_eq!(class("<unknown-el"), VueComponentClass::Resolved);
    assert_eq!(class("<div"), VueComponentClass::None);

    let facts = |text: &str| snapshot.component_tag(component.node(&topology, text));
    assert_eq!(
        facts("<KeepAlive").built_in,
        Some(BuiltinComponent::KeepAlive)
    );
    assert_eq!(
        facts("<Child").binding,
        Some(VueComponentBindingMatch {
            name: "Child".to_string(),
            member_tail: String::new(),
        })
    );
    assert_eq!(facts("<App").binding, None);
    assert_eq!(facts("<unknown-el").binding, None);
}

#[test]
fn a_built_in_tag_outranks_a_binding_named_like_it() {
    let component = Component::new("<Suspense />");
    let topology = component.topology();
    let allocator = Allocator::default();
    let snapshot = build(
        inputs(
            &component.source,
            Some(&topology),
            &[("Suspense", BindingType::SetupConst)],
            &allocator,
        ),
        &[client_tree()],
    )
    .expect("snapshot");
    let facts = snapshot.component_tag(component.node(&topology, "<Suspense"));
    assert_eq!(facts.class, VueComponentClass::BuiltIn);
    assert_eq!(facts.built_in, Some(BuiltinComponent::Suspense));
    assert_eq!(facts.binding, None);
}

#[test]
fn a_tag_does_not_match_a_binding_that_differs_only_in_leading_case() {
    let component = Component::new("<Foo />");
    let topology = component.topology();
    let allocator = Allocator::default();
    let snapshot = build(
        inputs(
            &component.source,
            Some(&topology),
            &[("foo", BindingType::SetupConst)],
            &allocator,
        ),
        &[client_tree()],
    )
    .expect("snapshot");
    let facts = snapshot.component_tag(component.node(&topology, "<Foo"));
    assert_eq!(facts.class, VueComponentClass::Resolved);
    assert_eq!(facts.binding, None);
}

#[test]
fn a_member_tag_records_the_binding_root_and_its_tail() {
    let component = Component::new("<Swiper.Item /><my-header />");
    let topology = component.topology();
    let allocator = Allocator::default();
    let snapshot = build(
        inputs(
            &component.source,
            Some(&topology),
            &[
                ("Swiper", BindingType::SetupConst),
                ("myHeader", BindingType::SetupConst),
            ],
            &allocator,
        ),
        &[client_tree()],
    )
    .expect("snapshot");
    let matched = |text: &str| {
        snapshot
            .component_tag(component.node(&topology, text))
            .binding
            .clone()
            .expect("a matched binding")
    };
    assert_eq!(
        matched("<Swiper.Item"),
        VueComponentBindingMatch {
            name: "Swiper".to_string(),
            member_tail: ".Item".to_string(),
        }
    );
    assert_eq!(
        matched("<my-header"),
        VueComponentBindingMatch {
            name: "myHeader".to_string(),
            member_tail: String::new(),
        }
    );
}

#[test]
fn a_kebab_tag_resolves_a_pascal_binding() {
    let component = Component::new("<my-child />");
    let topology = component.topology();
    let allocator = Allocator::default();
    let snapshot = build(
        inputs(
            &component.source,
            Some(&topology),
            &[("MyChild", BindingType::SetupConst)],
            &allocator,
        ),
        &[client_tree()],
    )
    .expect("snapshot");
    assert_eq!(
        snapshot.component_class(component.node(&topology, "<my-child")),
        VueComponentClass::Binding
    );
}

#[test]
fn refuses_incomplete_topology_for_template_family() {
    let component = Component::new("<div><span></div>");
    let topology = component.topology();
    assert_eq!(topology.completeness(), RegionCompleteness::Incomplete);
    let allocator = Allocator::default();
    reset_parse_template_expressions_calls();

    let refusal = build(
        inputs(&component.source, Some(&topology), &[], &allocator),
        &[client_tree()],
    )
    .err();

    assert_eq!(refusal, Some(VueSemanticRefusal::IncompleteTopology));
    assert_eq!(parse_template_expressions_call_count(), 0);
}

#[test]
fn a_demanded_template_family_needs_the_topology() {
    let allocator = Allocator::default();
    let refusal = build(inputs("", None, &[], &allocator), &[client_tree()]).err();
    assert_eq!(
        refusal,
        Some(VueSemanticRefusal::MissingInput {
            family: FactFamily::Template
        })
    );
}

#[test]
fn undemanded_imports_family_builds_no_origin_view() {
    let component = Component::new("<p />");
    let topology = component.topology();
    let allocator = Allocator::default();
    let before = origin_view_build_count();

    // Server output reads no reactivity, so it demands no imports.
    let server_snapshot = build(
        inputs(&component.source, Some(&topology), &[], &allocator),
        &[server()],
    )
    .expect("snapshot");
    assert!(!server_snapshot.families().contains(FactFamily::Imports));
    assert_eq!(origin_view_build_count(), before);

    let client_snapshot = build(
        inputs(&component.source, Some(&topology), &[], &allocator),
        &[client_tree()],
    )
    .expect("snapshot");
    assert!(client_snapshot.families().contains(FactFamily::Imports));
    assert_eq!(origin_view_build_count(), before + 1);
}

#[test]
fn demanded_bindings_carry_their_callee_origin_and_reported_class() {
    let setup = "import { ref as r } from 'vue'\nconst n = r(0)\nconst m = make()";
    let analysis_allocator = Allocator::default();
    let analysis = build_script_analysis_with_scope(
        setup,
        SourceType::ts(),
        &analysis_allocator,
        AnalysisScope::BUILD,
    );
    let component = Component::new("<p />");
    let topology = component.topology();
    let allocator = Allocator::default();
    let mut inputs = inputs(
        &component.source,
        Some(&topology),
        &[
            ("n", BindingType::SetupRef),
            ("m", BindingType::SetupMaybeRef),
        ],
        &allocator,
    );
    inputs.setup_analysis = Some(&analysis);

    let snapshot = build(inputs, &[client_tree()]).expect("snapshot");

    assert!(snapshot.families().contains(FactFamily::Reactivity));
    let n = snapshot.bindings().get("n").expect("n");
    assert_eq!(n.binding, BindingType::SetupRef);
    assert_eq!(
        n.callee_origin,
        FrameworkOrigin::Api(FrameworkApi::Ref, OriginEvidence::NamedImport)
    );
    let m = snapshot.bindings().get("m").expect("m");
    assert_eq!(
        (m.binding, m.reported, m.callee_origin),
        (
            BindingType::SetupMaybeRef,
            BindingType::SetupMaybeRef,
            FrameworkOrigin::Unknown
        )
    );
    assert_eq!(snapshot.origins().callee_api("r"), Some(FrameworkApi::Ref));
}
