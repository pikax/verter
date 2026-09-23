use super::public_constructor::*;
use super::script_setup::{ScriptBlockInput, SetupProjectionRefusal};
use crate::cursor::ScriptLanguage;

fn block(content: &str) -> ScriptBlockInput<'_> {
    ScriptBlockInput {
        content,
        content_start: 0,
        lang: Some(ScriptLanguage::TypeScript),
    }
}

fn project(setup: &str, generic: Option<&str>) -> VuePublicConstructorContract {
    project_public_constructor(None, Some(block(setup)), generic).expect("projects")
}

fn declaration(contract: &VuePublicConstructorContract) -> String {
    contract.declaration().expect("setup renders a constructor")
}

const PICKER_GENERIC: &str = "const T extends string | number = string";

const PICKER_SETUP: &str = r#"import { ref, type VNode } from "vue";
const props = defineProps<{ test: T; label?: string }>();
const emit = defineEmits<{ change: [value: T]; close: [] }>();
defineSlots<{ default(props: { item: T }): VNode[] }>();
const open = defineModel<boolean>("open");
const secret = ref(0);
const current = ref<T>();
function reset(): void { secret.value = 0; }
defineExpose({ reset, current });
defineOptions({ name: "Picker", inheritAttrs: false });
"#;

fn picker() -> VuePublicConstructorContract {
    project(PICKER_SETUP, Some(PICKER_GENERIC))
}

/// The construct-signature members of the rendered default export.
fn constructor_members(rendered: &str) -> Vec<&str> {
    let start = rendered
        .find("declare const __VerterPublicComponent: {\n")
        .expect("constructor value");
    rendered[start..]
        .lines()
        .skip(1)
        .take_while(|line| *line != "};")
        .map(str::trim)
        .collect()
}

#[test]
fn picker_probe_fixture_is_the_rendered_declaration() {
    const FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/sfc-projection/STP16/probes/components/Picker.vue.d.ts"
    ));
    let fixture = FIXTURE.replace("\r\n", "\n");
    let rendered = declaration(&picker());
    assert!(
        fixture.ends_with(&rendered),
        "the probe fixture must end with the rendered declaration:\n{rendered}"
    );
}

#[test]
fn required_props_keep_one_constructor_with_a_required_argument() {
    let contract = picker();
    assert_eq!(contract.source, ConstructorSource::ScriptSetup);
    assert_eq!(contract.props_requirement, PropsRequirement::Required);
    let rendered = declaration(&contract);
    assert!(rendered.ends_with("export default __VerterPublicComponent;\n"));
    let members = constructor_members(&rendered);
    assert_eq!(
        members[0],
        "new <const T extends string | number = string>(props: __VerterPublicProps<T>): __VerterPublicInstance<T>;"
    );

    // Clean twins: every prop optional keeps the argument optional; a props
    // type named from another module leaves the decision to TypeScript.
    let optional = project("defineProps<{ label?: string }>();\n", None);
    assert_eq!(optional.props_requirement, PropsRequirement::Optional);
    assert!(declaration(&optional)
        .contains("new (props?: __VerterPublicProps): __VerterPublicInstance;"));
    let imported = project(
        "import type { Props } from './props';\ndefineProps<Props>();\n",
        None,
    );
    assert_eq!(imported.props_requirement, PropsRequirement::Undetermined);
    assert!(declaration(&imported).contains(
        "new (...args: {} extends __VerterPublicProps ? [props?: __VerterPublicProps] : [props: __VerterPublicProps]): __VerterPublicInstance;"
    ));
    // Local declarations decide syntactically, including a required model.
    let local = project(
        "interface Props { id: number }\ndefineProps<Props>();\n",
        None,
    );
    assert_eq!(local.props_requirement, PropsRequirement::Required);
    let model = project(
        "defineProps<{ label?: string }>();\ndefineModel<string>({ required: true });\n",
        None,
    );
    assert_eq!(model.props_requirement, PropsRequirement::Required);
    assert!(declaration(&model).contains("\"modelValue\": string; \"modelModifiers\"?:"));
    let runtime = project(
        "defineProps({ id: { type: Number, required: true } });\n",
        None,
    );
    assert_eq!(runtime.props_requirement, PropsRequirement::Required);
    assert!(declaration(&runtime)
        .starts_with("const __VerterRuntimeProps = ({ id: { type: Number, required: true } });\n"));
}

#[test]
fn instance_publishes_typed_framework_and_exposed_members() {
    let contract = picker();
    let members: Vec<(&str, InstanceMemberOrigin)> = contract
        .instance
        .members
        .iter()
        .map(|member| (member.name.as_str(), member.origin))
        .collect();
    assert_eq!(
        members,
        vec![
            ("$props", InstanceMemberOrigin::Framework),
            ("$emit", InstanceMemberOrigin::Framework),
            ("$slots", InstanceMemberOrigin::Framework),
            ("reset", InstanceMemberOrigin::Exposed),
            ("current", InstanceMemberOrigin::Exposed),
        ]
    );
    let rendered = declaration(&contract);
    // Model events and listeners join the declared events.
    assert!(rendered.contains("((event: \"update:open\", value: boolean) => void)"));
    assert!(rendered.contains("\"onUpdate:open\"?: (value: boolean) => void"));
    assert!(rendered.contains(
        "import(\"vue\").EmitsToProps<import(\"vue\").TypeEmitsToOptions<({ change: [value: T]; close: [] })>>"
    ));
    assert_eq!(contract.options.name.as_deref(), Some("Picker"));
    assert_eq!(contract.options.inherit_attrs, Some(false));
    assert_eq!(
        &constructor_members(&rendered)[1..],
        [
            "readonly name: \"Picker\";",
            "readonly inheritAttrs: false;"
        ]
    );
}

#[test]
fn private_setup_bindings_stay_off_the_instance() {
    let contract = picker();
    assert!(!contract.instance.publishes("secret"));
    assert_eq!(
        contract.instance.private_bindings,
        vec!["ref", "props", "emit", "open", "secret"]
    );
    assert!(!declaration(&contract).contains("secret"));
    // A setup without `defineExpose` keeps the instance closed.
    let closed = project("const secret = 1;\ndefineProps<{ a?: string }>();\n", None);
    assert_eq!(closed.instance.private_bindings, vec!["secret"]);
    assert!(!declaration(&closed).contains("__VerterExpose"));
}

#[test]
fn generic_constraints_stay_on_the_single_construct_signature() {
    let contract = project(
        "defineProps<{ a: A; b?: B }>();\n",
        Some("A extends object, const B extends keyof A = keyof A"),
    );
    let receipt = contract.receipt();
    assert_eq!(receipt.construct_signatures, 1);
    assert_eq!(receipt.call_signatures, 0);
    assert_eq!(receipt.binder_params, vec!["A", "B"]);
    let rendered = declaration(&contract);
    let members = constructor_members(&rendered);
    assert_eq!(
        members.len(),
        1,
        "exactly one construct signature: {members:?}"
    );
    assert!(members[0].starts_with(
        "new <A extends object, const B extends keyof A = keyof A>(props: __VerterPublicProps<A, B>)"
    ));
    // Aliases carry the constraints without `const`, which aliases reject.
    assert!(rendered
        .contains("type __VerterPublicProps<A extends object, B extends keyof A = keyof A> ="));
    assert!(
        !rendered.contains("any"),
        "no permissive signature: {rendered}"
    );
}

#[test]
fn default_export_is_never_callable() {
    let rendered = declaration(&picker());
    for member in constructor_members(&rendered) {
        assert!(
            member.starts_with("new ") || member.starts_with("readonly "),
            "the default export is constructor-shaped only: {member}"
        );
    }
    // Without `<script setup>` the authored default (Vue's constructor-typed
    // `defineComponent`) is the constructor; nothing replaces it.
    let options = project_public_constructor(
        Some(block(
            "export default { name: 'Plain', inheritAttrs: false };\n",
        )),
        None,
        None,
    )
    .expect("projects");
    assert_eq!(options.source, ConstructorSource::AuthoredDefault);
    assert_eq!(options.declaration(), None);
    assert_eq!(options.receipt().construct_signatures, 0);
    assert_eq!(options.options.name.as_deref(), Some("Plain"));
}

#[test]
fn rendered_fields_keep_authored_types() {
    let contract = picker();
    let rendered = declaration(&contract);
    assert!(rendered.contains("({ test: T; label?: string })"));
    assert!(rendered.contains("\"open\"?: boolean"));
    assert!(
        rendered.contains("readonly $slots: Readonly<{ default(props: { item: T }): VNode[] }>")
    );
    for vacuous in ["any", "unknown", "never"] {
        assert!(
            !rendered.contains(vacuous),
            "no {vacuous} beyond the authored text: {rendered}"
        );
    }
    assert!(contract.receipt().open_domains.is_empty());
    // Framework-legal open domains keep Vue's own typing, and say so.
    let open = project("defineProps(['a']);\nconst m = defineModel();\n", None);
    assert_eq!(
        open.receipt().open_domains,
        vec![
            PublicSurface::Props,
            PublicSurface::Models,
            PublicSurface::Slots
        ]
    );
    assert!(declaration(&open).contains("{ readonly \"a\"?: any }"));
}

#[test]
fn binder_arguments_reach_every_public_surface() {
    let contract = picker();
    assert_eq!(
        contract.receipt().binder_dependent_surfaces,
        vec![
            PublicSurface::Props,
            PublicSurface::Events,
            PublicSurface::Slots,
            PublicSurface::Expose,
        ]
    );
    let rendered = declaration(&contract);
    for specialized in [
        "readonly $props: __VerterPublicProps<T>;",
        "): __VerterPublicInstance<T>;",
        "ReturnType<typeof __VerterExpose<T>>",
    ] {
        assert!(
            rendered.contains(specialized),
            "{specialized} in {rendered}"
        );
    }
    // A non-generic component renders without binder arguments.
    let plain = project("defineExpose({ a: 1 });\n", None);
    assert!(declaration(&plain).contains("ReturnType<typeof __VerterExpose>"));
    assert!(plain.receipt().binder_dependent_surfaces.is_empty());
}

#[test]
fn refuses_what_the_setup_projection_refuses() {
    assert_eq!(
        project_public_constructor(None, Some(block("defineProps<{ a: }>();")), None).unwrap_err(),
        SetupProjectionRefusal::SyntaxErrors { setup: true }
    );
    assert_eq!(
        project_public_constructor(None, Some(block("defineProps();")), Some("T extends"))
            .unwrap_err(),
        SetupProjectionRefusal::InvalidGeneric
    );
    // A local function named like the macro is an ordinary call.
    let shadowed = project(
        "function defineProps<T>(): T { return {} as T; }\ndefineProps<{ a: string }>();\n",
        None,
    );
    assert_eq!(shadowed.props, DeclaredSurface::None);
}
