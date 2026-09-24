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
        "/../../tests/sfc-projection/STP16/probes/components/Picker.vue.ts"
    ));
    let fixture = FIXTURE.replace("\r\n", "\n");
    let rendered = declaration(&picker());
    assert!(
        fixture.ends_with(&rendered),
        "the probe fixture must end with the rendered declaration:\n{rendered}"
    );
}

/// One probe component: its pinned declaration fixture and the authored
/// script pair it is rendered from.
struct RequirementProbe {
    name: &'static str,
    fixture: &'static str,
    normal: Option<&'static str>,
    setup: &'static str,
    generic: Option<&'static str>,
}

/// Probe components TypeScript checks at the constructor: `withDefaults`
/// defaults, same-name interfaces merged across both blocks with an
/// `as const` required model, and binder-dependent runtime options.
const REQUIREMENT_PROBES: [RequirementProbe; 3] = [
    RequirementProbe {
        name: "Defaulted.vue.ts",
        fixture: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/sfc-projection/STP16/probes/components/Defaulted.vue.ts"
        )),
        normal: None,
        setup: "withDefaults(defineProps<{ test: string; label?: string }>(), { test: \"x\" });\n",
        generic: None,
    },
    RequirementProbe {
        name: "Merged.vue.ts",
        fixture: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/sfc-projection/STP16/probes/components/Merged.vue.ts"
        )),
        normal: Some("interface Props { label?: string }\n"),
        setup: "interface Props { id: number }\ndefineProps<Props>();\ndefineModel<string>({ required: true as const });\n",
        generic: None,
    },
    RequirementProbe {
        name: "Strict.vue.ts",
        fixture: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/sfc-projection/STP16/probes/components/Strict.vue.ts"
        )),
        normal: None,
        setup: "import type { PropType } from \"vue\";\ndefineProps({ value: { type: String as unknown as PropType<T>, required: true as const } });\ndefineEmits({ change: (payload: T) => true });\n",
        generic: Some("T extends string"),
    },
];

#[test]
fn requirement_probe_fixtures_are_the_rendered_declarations() {
    for probe in REQUIREMENT_PROBES {
        let contract = project_public_constructor(
            probe.normal.map(block),
            Some(block(probe.setup)),
            probe.generic,
        )
        .expect("projects");
        let rendered = declaration(&contract);
        assert!(
            probe.fixture.replace("\r\n", "\n").ends_with(&rendered),
            "{} must end with the rendered declaration:\n{rendered}",
            probe.name
        );
    }
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
    // The hoisted options satisfy Vue's props options type, so `required:
    // true` stays the literal `true` that makes the public prop required.
    assert!(declaration(&runtime).starts_with(
        "const __VerterRuntimeProps = (({ id: { type: Number, required: true } }) satisfies import(\"vue\").ComponentObjectPropsOptions);\n"
    ));
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
    // The provider body reads `secret`; the public aliases never name it.
    let rendered = declaration(&contract);
    let public = &rendered[rendered.find("type __VerterPublicProps").expect("aliases")..];
    assert!(!public.contains("secret"), "{public}");
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
fn expose_provider_is_rendered_from_the_setup_statements() {
    let rendered = declaration(&picker());
    // One provider over the binder: the setup statements (imports stay at
    // module scope) returning the `defineExpose` argument.
    let provider = &rendered[rendered
        .find("function __VerterExpose<const T extends string | number = string>() {\n")
        .expect("provider is declared in the declaration")..];
    assert!(!provider.contains("import "), "{provider}");
    assert!(provider.contains("const current = ref<T>();\n"));
    assert!(provider.contains("return ({ reset, current });\n}\n"));
    assert_eq!(rendered.matches("function __VerterExpose").count(), 1);
    // Top-level await makes the provider async and the instance awaits it;
    // exported setup declarations stay function-local.
    let awaited = project(
        "export interface Row { id: number }\nconst rows: Row[] = await load();\ndefineExpose({ rows });\n",
        None,
    );
    let rendered = declaration(&awaited);
    assert!(rendered.starts_with(
        "async function __VerterExpose() {\ninterface Row { id: number }\nconst rows: Row[] = await load();\n"
    ));
    assert!(rendered.contains("ShallowUnwrapRef<Awaited<ReturnType<typeof __VerterExpose>>>"));
}

#[test]
fn with_defaults_makes_defaulted_props_omissible() {
    let defaulted = project(
        "withDefaults(defineProps<{ test: string; label?: string }>(), { test: 'x' });\n",
        None,
    );
    assert_eq!(defaulted.props_requirement, PropsRequirement::Optional);
    assert_eq!(
        defaulted
            .props_defaults
            .as_ref()
            .and_then(|d| d.keys.clone()),
        Some(vec!["test".to_string()])
    );
    let rendered = declaration(&defaulted);
    assert!(rendered
        .contains("__VerterPropsWithDefaults<({ test: string; label?: string }), \"test\">"));
    assert!(rendered.contains("new (props?: __VerterPublicProps): __VerterPublicInstance;"));
    // A prop without a default stays required.
    let partial = project(
        "const props = withDefaults(defineProps<{ a: string; b: number }>(), { a: 'x' });\n",
        None,
    );
    assert_eq!(partial.props_requirement, PropsRequirement::Required);
    // Defaults whose key set is open are merged at runtime: Vue keeps every
    // authored `required`, so a prop the spread may not cover (`id`) stays
    // required and no key is remapped through a widened `keyof`.
    for defaults in ["{ ...base }", "{ ...base, a: 'x' }", "base"] {
        let open = project(
            &format!(
                "import {{ base }} from './base';\nwithDefaults(defineProps<{{ a: string; id: number }}>(), {defaults});\n"
            ),
            None,
        );
        assert_eq!(
            open.props_requirement,
            PropsRequirement::Required,
            "{defaults}"
        );
        assert_eq!(open.props_defaults.as_ref().map(|d| &d.keys), Some(&None));
        let rendered = declaration(&open);
        assert!(
            !rendered.contains("__VerterPropsWithDefaults"),
            "{rendered}"
        );
        assert!(rendered.starts_with(
            "type __VerterPublicProps = import(\"vue\").PublicProps & ({ a: string; id: number });\n"
        ));
        assert!(rendered.contains("new (props: __VerterPublicProps): __VerterPublicInstance;"));
    }
}

#[test]
fn required_flags_read_through_const_assertions() {
    for setup in [
        "defineModel<string>({ required: true as const });\n",
        "defineModel<string>({ required: <const>true });\n",
        "defineModel<string>({ required: (true satisfies boolean) });\n",
        "defineProps({ id: { type: String, required: true as const } });\n",
    ] {
        assert_eq!(
            project(setup, None).props_requirement,
            PropsRequirement::Required,
            "{setup}"
        );
    }
    let model = project("defineModel<string>({ required: true as const });\n", None);
    assert_eq!(model.models[0].required, PropsRequirement::Required);
    assert!(declaration(&model).contains("{ \"modelValue\": string;"));
    // A value whose type may be `boolean` is TypeScript's decision.
    let widened = project(
        "import { strict } from './flags';\ndefineModel<string>({ required: strict });\n",
        None,
    );
    assert_eq!(widened.props_requirement, PropsRequirement::Undetermined);
    let rendered = declaration(&widened);
    assert!(rendered.starts_with("const __VerterModelRequired0 = (strict);\n"));
    assert!(rendered.contains(
        "(typeof __VerterModelRequired0 extends true ? { \"modelValue\": string } : { \"modelValue\"?: string })"
    ));
    assert!(rendered.contains("new (...args: {} extends __VerterPublicProps ?"));
    // The flag is read through the options object's own parentheses,
    // `as const` and `satisfies`.
    for options in [
        "({ required: true as const })",
        "{ required: true as const } as const",
        "{ required: true } satisfies { required: true }",
        "({ required: true })!",
    ] {
        let model = project(&format!("defineModel<string>({options});\n"), None);
        assert_eq!(
            model.models[0].required,
            PropsRequirement::Required,
            "{options}"
        );
    }
    // A spread that may set `required`, or an options value that is not an
    // object literal, leaves the decision to TypeScript over the hoisted
    // options; a spread keeps only the members that decide `required`.
    for (setup, hoisted) in [
        (
            "defineModel<string>({ ...{ required: true as const }, get(v) { return v; } });\n",
            "const __VerterModelOptions0 = (({ ...{ required: true as const } }) satisfies { readonly required?: boolean; readonly [key: string]: unknown });\n",
        ),
        (
            "import { flags } from './flags';\ndefineModel<string>({ required: false, ...flags });\n",
            "const __VerterModelOptions0 = (({ required: false, ...flags }) satisfies { readonly required?: boolean; readonly [key: string]: unknown });\n",
        ),
        (
            "import { opts } from './flags';\ndefineModel<string>(\"count\", opts);\n",
            "const __VerterModelOptions0 = (opts);\n",
        ),
        (
            "import { opts, Options } from './flags';\ndefineModel<string>(opts as Options);\n",
            "const __VerterModelOptions0 = (opts as Options);\n",
        ),
    ] {
        let model = project(setup, None);
        assert_eq!(model.models[0].required, PropsRequirement::Undetermined, "{setup}");
        let rendered = declaration(&model);
        assert!(rendered.starts_with(hoisted), "{rendered}");
        let key = if setup.contains("\"count\"") { "count" } else { "modelValue" };
        assert!(
            rendered.contains(&format!(
                "(typeof __VerterModelOptions0 extends {{ required: true }} ? {{ \"{key}\": string }} : {{ \"{key}\"?: string }})"
            )),
            "{rendered}"
        );
        assert!(rendered.contains("new (...args: {} extends __VerterPublicProps ?"));
    }
    // A later `required` overrides an earlier spread and is read directly.
    let overridden = project(
        "import { flags } from './flags';\ndefineModel<string>({ ...flags, required: true });\n",
        None,
    );
    assert_eq!(overridden.models[0].required, PropsRequirement::Required);
    let runtime = project(
        "import { strict } from './flags';\ndefineProps({ id: { type: String, required: strict } });\n",
        None,
    );
    assert_eq!(runtime.props_requirement, PropsRequirement::Undetermined);
    let spread = project(
        "import { req } from './flags';\ndefineProps({ id: { type: String, ...req } });\n",
        None,
    );
    assert_eq!(spread.props_requirement, PropsRequirement::Undetermined);
}

#[test]
fn hoisted_values_carry_the_setup_literal_constants_they_reference() {
    // Vue hoists a setup `const` with a static initializer to module scope,
    // so a hoisted value may reference it; its declaration is hoisted too.
    let model = project(
        "const strict = true as const;\nconst label: string = `x`, count = 1;\nconst open = ref(0);\ndefineModel<string>({ required: strict });\ndefineProps({ n: { type: Number, default: count } });\n",
        None,
    );
    assert_eq!(model.models[0].required, PropsRequirement::Undetermined);
    let rendered = declaration(&model);
    assert!(
        rendered.starts_with(
            "const strict = true as const;\nconst count = 1;\nconst __VerterModelRequired0 = (strict);\n"
        ),
        "{rendered}"
    );
    assert!(!rendered.contains("const label"), "{rendered}");
    assert!(!rendered.contains("const open"), "{rendered}");
    // With a normal script Vue does not hoist setup constants, so nothing
    // setup-local is rendered at module scope.
    let split = project_public_constructor(
        Some(block("export default {};\n")),
        Some(block(
            "const strict = true as const;\ndefineModel<string>({ required: strict });\n",
        )),
        None,
    )
    .expect("projects");
    assert!(declaration(&split).starts_with("const __VerterModelRequired0 = (strict);\n"));
}

#[test]
fn merged_interfaces_join_their_requirements() {
    let merged = project(
        "interface Props { label?: string }\ninterface Props { id: number }\ndefineProps<Props>();\n",
        None,
    );
    assert_eq!(merged.props_requirement, PropsRequirement::Required);
    // The merge spans the normal script and the setup block.
    let split = project_public_constructor(
        Some(block("interface Props { label?: string }\n")),
        Some(block(
            "interface Props { id: number }\ndefineProps<Props>();\n",
        )),
        None,
    )
    .expect("projects");
    assert_eq!(split.props_requirement, PropsRequirement::Required);
    let optional = project(
        "interface Props { label?: string }\ninterface Props { id?: number }\ndefineProps<Props>();\n",
        None,
    );
    assert_eq!(optional.props_requirement, PropsRequirement::Optional);
}

#[test]
fn binder_dependent_runtime_options_are_rendered_over_the_binder() {
    let contract = project(
        "import type { PropType } from 'vue';\ndefineProps({ value: { type: String as PropType<T>, required: true } });\ndefineEmits({ change: (payload: T) => true });\n",
        Some("T extends string"),
    );
    assert_eq!(
        contract.receipt().binder_dependent_surfaces,
        vec![PublicSurface::Props, PublicSurface::Events]
    );
    let rendered = declaration(&contract);
    assert!(rendered.starts_with(
        "const __VerterRuntimeProps = <T extends string,>() => (({ value: { type: String as PropType<T>, required: true } }) satisfies import(\"vue\").ComponentObjectPropsOptions);\nconst __VerterRuntimeEmits = <T extends string,>() => ({ change: (payload: T) => true });\n"
    ));
    assert!(rendered.contains(
        "import(\"vue\").ExtractPublicPropTypes<ReturnType<typeof __VerterRuntimeProps<T>>>"
    ));
    assert!(rendered.contains("import(\"vue\").EmitFn<ReturnType<typeof __VerterRuntimeEmits<T>>>"));
    // Options that name no binder parameter stay plain module constants.
    let plain = project("defineProps({ id: Number });\n", Some("T extends string"));
    assert!(declaration(&plain).starts_with(
        "const __VerterRuntimeProps = (({ id: Number }) satisfies import(\"vue\").ComponentObjectPropsOptions);\n"
    ));
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
