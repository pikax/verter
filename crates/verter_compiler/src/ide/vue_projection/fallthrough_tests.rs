//! Root contracts projected from retained topology and public script facts.
use super::attribute_operations::{project_attribute_operations, AttributeOperationsProjection};
use super::branches::{project_template_branches, TemplateBranchProjection};
use super::builtins::{project_builtins, BuiltinProjection, CarrierShape};
use super::component_resolution::{project_component_resolution, ComponentResolutionProjection};
use super::component_use::project_component_uses;
use super::fallthrough::*;
use super::public_constructor::{
    project_public_constructor, PublicDeclarationExtras, VuePublicConstructorContract,
};
use super::script_setup::{project_script_pair, ScriptBlockInput};
use crate::cursor::ScriptLanguage;
use crate::framework_common::projection_plan::{build_projection_plan, PlanInput, ProjectionPlan};
use crate::template::topology::{build_template_topology, TopologyRecoveryFacts};
use crate::work_ledger::CompilerWorkLedger;
use verter_audit::observe::Uncaptured;

struct Inputs {
    plan: ProjectionPlan,
    branches: TemplateBranchProjection,
    resolution: ComponentResolutionProjection,
    public: VuePublicConstructorContract,
    builtins: BuiltinProjection,
    attributes: AttributeOperationsProjection,
}
impl Inputs {
    fn new(setup: &str, template: &str) -> Self {
        let source =
            format!("<script setup lang=\"ts\">{setup}</script><template>{template}</template>");
        let parsed = crate::framework_common::vue_bridge::parse_sfc(&source, None, None);
        let plan = build_projection_plan(PlanInput {
            canonical_id: "file:///src/Panel.vue",
            source: &source,
            parsed: &parsed,
            parse_key: None,
            syntax_profile: None,
            custom_elements: None,
        });
        let topology = build_template_topology(
            &source,
            parsed.template_ast().unwrap(),
            TopologyRecoveryFacts::default(),
            &mut CompilerWorkLedger::<Uncaptured>::new(),
        )
        .unwrap();
        let branches = project_template_branches(&plan, &topology);
        let attributes = project_attribute_operations(&plan, &parsed, &source);
        let uses = project_component_uses(&plan, &attributes);
        let block = || ScriptBlockInput {
            content: setup,
            content_start: 0,
            lang: Some(ScriptLanguage::TypeScript),
        };
        let script = project_script_pair(None, Some(block()), None).unwrap();
        let public = project_public_constructor(None, Some(block()), None).unwrap();
        let resolution =
            project_component_resolution(&plan, &uses, &script, "file:///src/Panel.vue", None);
        let builtins = project_builtins(&plan, &resolution, CarrierShape::of(&parsed));
        Self {
            plan,
            branches,
            resolution,
            public,
            builtins,
            attributes,
        }
    }
    fn project(&self) -> FallthroughProjection {
        project_fallthrough(
            &self.plan,
            &self.branches,
            &self.resolution,
            &self.public,
            &self.builtins,
            &self.attributes,
        )
    }
    fn declaration(&self) -> String {
        let product = self.project();
        assert!(product.complete && !product.pending, "{product:?}");
        let mut text = product
            .preludes()
            .iter()
            .map(|prelude| prelude.text)
            .collect::<String>();
        text.push_str(
            &self
                .public
                .declaration_over("__Scope", &product.declaration_extras())
                .unwrap(),
        );
        let allocator = oxc_allocator::Allocator::default();
        let parsed =
            verter_parser::oxc_parse::Parser::new(&allocator, &text, oxc_span::SourceType::ts())
                .parse();
        assert!(
            parsed.diagnostics.is_empty(),
            "{:?}\n{text}",
            parsed.diagnostics
        );
        text
    }
}

#[test]
fn fallthrough_default_and_explicit_inheritance_agree() {
    let implicit = Inputs::new("", "<a/>").project();
    let explicit = Inputs::new("defineOptions({ inheritAttrs: true });", "<a/>").project();
    assert_eq!(implicit.fallthrough, explicit.fallthrough);
    assert_eq!(implicit.attrs, explicit.attrs);
    assert_eq!(implicit.roots.shape, RootShape::Single);
    let disabled = Inputs::new("defineOptions({ inheritAttrs: false });", "<a/>").project();
    assert!(disabled.fallthrough.props_type.is_none());
    assert!(disabled
        .attrs
        .attrs_type
        .contains("__VerterNativeAttributes<\"a\""));
}

#[test]
fn fallthrough_fragments_text_loops_and_empty_templates_publish_no_native_props() {
    for (template, shape) in [
        ("<div/><span/>", RootShape::Multiple),
        ("<div/>text", RootShape::Multiple),
        ("{{ 1 }}<div/>", RootShape::Multiple),
        ("<div v-for=\"item in [1, 2]\"/>", RootShape::Multiple),
        ("<!-- comment -->", RootShape::None),
        ("", RootShape::None),
    ] {
        let product = Inputs::new("", template).project();
        assert!(product.complete, "{template}: {product:?}");
        assert_eq!(product.roots.shape, shape, "{template}");
        assert!(product.fallthrough.props_type.is_none(), "{template}");
    }
}

#[test]
fn fallthrough_conditional_roots_keep_a_union_including_template_arm_roots() {
    let inputs = Inputs::new(
        "defineProps<{ choose: boolean }>();",
        "<template v-if=\"choose\"><input/></template><a v-else/>",
    );
    let product = inputs.project();
    assert!(product.complete, "{product:?}");
    assert_eq!(product.roots.shape, RootShape::Conditional);
    assert_eq!(
        product.roots.roots.len(),
        0,
        "bare native roots require no checking host"
    );
    let props = product.fallthrough.props_type.unwrap();
    assert!(props.contains("__VerterNativeAttributes<\"input\", \"html\"> | __VerterNativeAttributes<\"a\", \"html\">"), "{props}");
    inputs.declaration();
}

#[test]
fn fallthrough_subtracts_declared_props_events_and_consumed_root_keys() {
    let inputs = Inputs::new(
        "defineProps<{ title: number }>(); defineEmits<{ save: [value: number] }>();",
        "<div id=\"fixed\" :tabindex=\"0\" class=\"merged\"/>",
    );
    let product = inputs.project();
    let props = product.fallthrough.props_type.unwrap();
    assert!(
        props.contains("Omit<__VerterNativeAttributes<\"div\", \"html\">, Exclude<__VerterNativeKey<\"id\"> | __VerterNativeKey<\"tabindex\">, \"class\" | \"style\">>"),
        "{props}"
    );
    assert!(props.contains("({ title: number })"), "{props}");
    assert!(props.contains("EmitsToProps"), "{props}");
    let declaration = inputs.declaration();
    assert!(
        declaration.contains(
            "type __VerterPublicProps = import(\"vue\").PublicProps & ({ title: number })"
        ),
        "{declaration}"
    );
}

#[test]
fn fallthrough_component_roots_withhold_unspecialized_declaration_extras() {
    for (setup, template) in [
        ("import Child from './Child.vue';", "<Child/>"),
        (
            "import Child from './Child.vue'; defineProps<{ value: number }>();",
            "<Child :value=\"value\"/>",
        ),
        (
            "import Child from './Child.vue'; defineProps<{ choose: boolean }>();",
            "<Child v-if=\"choose\"/><div v-else/>",
        ),
    ] {
        let product = Inputs::new(setup, template).project();
        assert!(!product.complete, "{product:?}");
        assert_eq!(
            product.declaration_extras(),
            PublicDeclarationExtras::default()
        );
        assert!(product.preludes().is_empty());
    }
}

/// A component among several roots is not a fallthrough root: the fragment's
/// contract is published without any reference to the child, exactly as for
/// a fragment of elements.
#[test]
fn fallthrough_fragments_holding_components_publish_the_fragment_contract() {
    const SETUP: &str = "import Child from './Child.vue';";
    let fragment_of_elements = Inputs::new(SETUP, "<div/><span/>").declaration();
    for template in [
        "<Child :initial-count=\"42\"/><Child :initial-count=\"1\"/>",
        "<Child/><div>{{ 1 }}</div>",
    ] {
        let inputs = Inputs::new(SETUP, template);
        let product = inputs.project();
        assert!(product.complete, "{template}: {product:?}");
        assert_eq!(product.roots.shape, RootShape::Multiple, "{template}");
        assert!(product.fallthrough.props_type.is_none(), "{template}");
        let declaration = inputs.declaration();
        assert!(!declaration.contains("typeof Child"), "{declaration}");
        assert_eq!(declaration, fragment_of_elements, "{template}");
    }
}

#[test]
fn fallthrough_native_root_edits_change_declarations() {
    let a = Inputs::new("", "<a/>").declaration();
    let div = Inputs::new("", "<div/>").declaration();
    assert_ne!(a, div);
    let nested =
        Inputs::new("import Child from './Child.vue';", "<div><Child/></div>").declaration();
    assert!(
        nested.contains("__VerterNativeAttributes<\"div\", \"html\">"),
        "{nested}"
    );
}

#[test]
fn fallthrough_refuses_stale_products_instead_of_publishing_declaration_extras() {
    let mut inputs = Inputs::new("", "<div/>");
    inputs.branches.snapshot = Inputs::new("", "<a/>").plan.snapshot;
    let product = inputs.project();
    assert!(!product.complete);
    assert_eq!(
        product.declaration_extras(),
        PublicDeclarationExtras::default()
    );
    assert!(product.preludes().is_empty());
}

#[test]
fn fallthrough_does_not_emit_module_references_to_setup_local_components() {
    let inputs = Inputs::new(
        "const Child = (props: { value?: number }) => null;",
        "<Child/>",
    );
    let product = inputs.project();
    assert!(!product.complete);
    assert_eq!(
        product.declaration_extras(),
        PublicDeclarationExtras::default()
    );
}

#[test]
fn fallthrough_consumed_component_keys_use_the_runtime_consumer_authority() {
    let inputs = Inputs::new(
        "import Child from './Child.vue';",
        "<Child :real-prop=\"42\"/>",
    );
    let props = inputs.project().fallthrough.props_type.unwrap();
    assert!(props.contains("__VerterUseDrop<"), "{props}");
    assert!(props.contains("\"realProp\" | \"real-prop\""), "{props}");
}

#[test]
fn fallthrough_unresolved_conditional_arms_withhold_the_whole_widening() {
    let inputs = Inputs::new(
        "defineProps<{ choose: boolean }>();",
        "<a v-if=\"choose\"/><slot v-else/>",
    );
    let product = inputs.project();
    assert!(!product.complete);
    assert_eq!(
        product.declaration_extras(),
        PublicDeclarationExtras::default()
    );
}

#[test]
fn fallthrough_consumes_model_value_listener_and_modifier_keys() {
    for (tag, expected, complete) in [
        (
            "Child",
            "__VerterUseDrop<__VerterUseComponentProps<__VerterUseContract<typeof Child>>, \"title\" | \"onUpdate:title\" | \"titleModifiers\">",
            false,
        ),
        (
            "div",
            "__VerterNativeKey<\"title\"> | __VerterNativeKey<\"onUpdate:title\"> | __VerterNativeKey<\"titleModifiers\">",
            true,
        ),
    ] {
        let inputs = Inputs::new(
            "import Child from './Child.vue'; let title = 'hello';",
            &format!("<{tag} v-model:title.trim=\"title\"/>"),
        );
        let product = inputs.project();
        assert_eq!(product.complete, complete, "{product:?}");
        let props = product.fallthrough.props_type.as_ref().unwrap();
        assert!(props.contains(expected), "{props}");
        if complete {
            inputs.declaration();
        } else {
            assert_eq!(
                product.declaration_extras(),
                PublicDeclarationExtras::default()
            );
        }
    }
}

#[test]
fn fallthrough_withholds_extras_from_stale_or_incomplete_attribute_products() {
    let mut inputs = Inputs::new("", "<a id=\"fixed\"/>");
    inputs.attributes.complete = false;
    assert!(!inputs.project().complete);
    assert_eq!(
        inputs.project().declaration_extras(),
        PublicDeclarationExtras::default()
    );
    inputs.attributes.complete = true;
    inputs.attributes.snapshot = Inputs::new("", "<div/>").plan.snapshot;
    assert!(!inputs.project().complete);
    assert_eq!(
        inputs.project().declaration_extras(),
        PublicDeclarationExtras::default()
    );
}
