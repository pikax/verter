//! Carriers composed into checking modules and checked by the engine.
//!
//! Each case composes `.vue` carriers with the projection composer, serves
//! the checking modules beside `packages/example` (whose `node_modules` holds
//! `vue`) through the in-memory overlay, and checks them with
//! [`super::typecheck`]. Diagnostics are compared as an exact
//! `(file, TS code)` multiset. Like the other engine suites in this crate,
//! a case reports SKIP and returns only when no gated engine or no installed
//! `vue` is present.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use verter_compiler::framework_common::registered_carrier_projection::project_registered_accepted;
use verter_compiler::framework_common::vue_projection_backend::{
    compose_checking_module, CheckingModuleInput,
};
use verter_compiler::framework_common::FrameworkParseArtifact;
use verter_language::carrier_grammar::{
    CarrierGrammarAuthority, CarrierParserGrammarVersion, FrameworkAdapterSemanticVersion,
};
use verter_language::registered_source_authority::{
    CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
};
use verter_span::path::canonicalize_path;
use verter_tsgo_api::toolchain::discovery::{resolve_blocking, ResolutionRequest};
use verter_tsgo_api::toolchain::validation::Capability;

use super::{typecheck, OverlayFile, RemapKind, TypecheckInputs};

/// One authored input of a check: a `.vue` carrier, composed, or a plain
/// TypeScript module served as written.
enum Input<'a> {
    Carrier(&'a str, &'a str),
    Module(&'a str, &'a str),
}

fn artifact(canonical: &str, source: &str) -> FrameworkParseArtifact {
    let language = verter_language::FileLanguage::vue();
    let sources = RegisteredSourceAuthority::new().expect("source authority");
    let snapshot = sources
        .register_source(
            CanonicalFileId::new(canonical),
            FileIncarnation::new(1),
            SourceGeneration::new(1),
            language.clone(),
            Arc::from(source),
        )
        .expect("registered source");
    let grammars = CarrierGrammarAuthority::new().expect("grammar authority");
    let config = verter_compiler::framework_common::vue_grammar_config(
        "{{",
        "}}",
        std::iter::empty::<&str>(),
    )
    .expect("vue");
    grammars
        .register_carrier_grammar(
            language,
            FrameworkAdapterSemanticVersion::new(1).expect("adapter version"),
            CarrierParserGrammarVersion::new(1).expect("grammar version"),
            config.clone(),
        )
        .expect("grammar registration");
    let accepted = grammars
        .accept_registered_source(&sources, &snapshot, &config)
        .expect("accepted source");
    project_registered_accepted(&accepted)
        .expect("registered projection")
        .into_framework_parse_artifact()
}

/// The project directory the overlay files are served in.
fn project_dir() -> PathBuf {
    let joined = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("packages")
        .join("example");
    // The engine opens the config under its resolved path, so the overlay must be
    // keyed by the same path: resolve the `..` segments rather than leave them in.
    joined.canonicalize().unwrap_or(joined)
}

/// Check `inputs` as one program; `None` (SKIP) when no engine or no `vue`.
/// Returns `(file name, TS code)` pairs, sorted.
fn check(inputs: &[Input<'_>]) -> Option<Vec<(String, u32)>> {
    check_with(&[], inputs)
}

/// [`check`] with further compiler options set to `true`.
fn check_with(flags: &[&str], inputs: &[Input<'_>]) -> Option<Vec<(String, u32)>> {
    let project = project_dir();
    if !project.join("node_modules").join("vue").exists() {
        eprintln!("SKIP: packages/example/node_modules/vue not found; run `pnpm install` first");
        return None;
    }
    let engine = match resolve_blocking(&ResolutionRequest::for_environment(
        Capability::Api,
        Some(project.clone()),
    )) {
        Ok(resolution) => resolution.path,
        Err(error) => {
            eprintln!("SKIP: no gated tsgo engine ({error:?})");
            return None;
        }
    };
    let dir = canonicalize_path(&project.to_string_lossy());
    let mut files = Vec::new();
    for input in inputs {
        let (path, content) = match input {
            Input::Carrier(name, source) => {
                let canonical = format!("{dir}/{name}");
                let artifact = artifact(&canonical, source);
                let module = compose_checking_module(&CheckingModuleInput {
                    source,
                    artifact: &artifact,
                    canonical_id: &canonical,
                    custom_elements: None,
                    external: None,
                    declarations: false,
                })
                .expect("the carrier composes");
                (
                    format!("{canonical}.{}", module.dialect.extension()),
                    module.checking_text().to_string(),
                )
            }
            Input::Module(name, source) => (format!("{dir}/{name}"), (*source).to_string()),
        };
        files.push(OverlayFile {
            path,
            content,
            remap: RemapKind::Passthrough,
        });
    }
    let tsconfig_path = format!("{dir}/tsconfig.projection-checks.json");
    let mut tsconfig = serde_json::json!({
        "compilerOptions": {
            "strict": true,
            "noEmit": true,
            "target": "ES2022",
            "module": "ESNext",
            "moduleResolution": "bundler",
            "skipLibCheck": true,
            "lib": ["ES2022", "DOM"],
            "types": []
        },
        "files": files.iter().map(|file| file.path.clone()).collect::<Vec<_>>(),
    });
    for flag in flags {
        tsconfig["compilerOptions"][*flag] = serde_json::Value::Bool(true);
    }
    let diagnostics = typecheck(TypecheckInputs {
        engine: &engine,
        cwd: &project,
        tsconfig_path,
        tsconfig_bytes: tsconfig.to_string(),
        files,
    })
    .expect("the engine checks the program");
    // Shown with a failing comparison: the code alone does not say which
    // statement failed.
    for diagnostic in &diagnostics {
        eprintln!(
            "{}({},{}): TS{} {}",
            diagnostic.file,
            diagnostic.line,
            diagnostic.col,
            diagnostic.ts_code,
            diagnostic.message
        );
    }
    let mut found: Vec<(String, u32)> = diagnostics
        .into_iter()
        .map(|diagnostic| {
            let name = diagnostic
                .file
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or_default()
                .to_string();
            (name, diagnostic.ts_code)
        })
        .collect();
    found.sort();
    Some(found)
}

const EXPOSING: &str = concat!(
    "<script setup lang=\"ts\">\n",
    "import { ref } from 'vue'\n",
    "const shown = ref(1);\n",
    "const hidden = ref('x');\n",
    "defineExpose({ shown });\n",
    "</script>\n",
);

#[test]
fn projection_route_instance_type_carries_only_exposed_members() {
    let reader = concat!(
        "import Exposing from './Exposing.vue';\n",
        "declare const instance: InstanceType<typeof Exposing>;\n",
        "export const shown: number = instance.shown;\n",
        "export const hidden = instance.hidden;\n",
    );
    let Some(found) = check(&[
        Input::Carrier("Exposing.vue", EXPOSING),
        Input::Module("reader.ts", reader),
    ]) else {
        return;
    };
    assert_eq!(found, vec![("reader.ts".to_string(), 2339)]);
}

#[test]
fn projection_route_top_level_await_keeps_the_instance_type_unwrapped() {
    let awaiting = concat!(
        "<script setup lang=\"ts\">\n",
        "const total = await Promise.resolve(1);\n",
        "defineExpose({ total });\n",
        "</script>\n",
    );
    let reader = concat!(
        "import Awaiting from './Awaiting.vue';\n",
        "declare const instance: InstanceType<typeof Awaiting>;\n",
        "export const total: number = instance.total;\n",
    );
    let Some(found) = check(&[
        Input::Carrier("Awaiting.vue", awaiting),
        Input::Module("awaitingReader.ts", reader),
    ]) else {
        return;
    };
    assert_eq!(found, Vec::new());
}

#[test]
fn projection_route_generic_setup_is_checked_under_its_constraint() {
    let clean = concat!(
        "<script setup lang=\"ts\" generic=\"T extends string\">\n",
        "const props = defineProps<{ value: T }>();\n",
        "const text: string = props.value;\n",
        "const n = <number>(text.length as unknown);\n",
        "</script>\n",
    );
    let wrong = concat!(
        "<script setup lang=\"ts\" generic=\"T extends string\">\n",
        "const props = defineProps<{ value: T }>();\n",
        "const count: number = props.value;\n",
        "</script>\n",
    );
    let Some(found) = check(&[
        Input::Carrier("GenericClean.vue", clean),
        Input::Carrier("GenericWrong.vue", wrong),
    ]) else {
        return;
    };
    assert_eq!(found, vec![("GenericWrong.vue.ts".to_string(), 2322)]);
}

#[test]
fn projection_route_setup_type_shadows_the_normal_script_type() {
    let shadowing = concat!(
        "<script lang=\"ts\">\n",
        "type A = 1;\n",
        "</script>\n",
        "<script setup lang=\"ts\">\n",
        "type A = 2;\n",
        "const a: A = 2;\n",
        "</script>\n",
    );
    let Some(found) = check(&[Input::Carrier("Shadowing.vue", shadowing)]) else {
        return;
    };
    assert_eq!(found, Vec::new());
}

#[test]
fn projection_route_template_reads_setup_refs_unwrapped() {
    let reading = concat!(
        "<template>\n  <p>{{ double(count) }}</p>\n</template>\n",
        "<script setup lang=\"ts\">\n",
        "import { ref } from 'vue'\n",
        "const count = ref<number>(1);\n",
        "const double = (n: number) => n * 2;\n",
        "</script>\n",
    );
    let wrong = concat!(
        "<template>\n  <p>{{ count.length }}</p>\n</template>\n",
        "<script setup lang=\"ts\">\n",
        "import { ref } from 'vue'\n",
        "const count = ref<number>(1);\n",
        "</script>\n",
    );
    let Some(found) = check(&[
        Input::Carrier("Reading.vue", reading),
        Input::Carrier("ReadingWrong.vue", wrong),
    ]) else {
        return;
    };
    assert_eq!(found, vec![("ReadingWrong.vue.ts".to_string(), 2339)]);
}

#[test]
fn projection_route_unknown_template_name_is_reported() {
    let unknown = concat!(
        "<template>\n  <p>{{ missing }}</p>\n</template>\n",
        "<script setup lang=\"ts\">\n",
        "const present = 1;\n",
        "</script>\n",
    );
    let Some(found) = check(&[Input::Carrier("Unknown.vue", unknown)]) else {
        return;
    };
    assert_eq!(found, vec![("Unknown.vue.ts".to_string(), 2339)]);
}

#[test]
fn projection_route_unused_setup_binding_is_reported_and_a_template_read_is_not() {
    let usage = concat!(
        "<template>\n  <p>{{ used }}</p>\n</template>\n",
        "<script setup lang=\"ts\">\n",
        "const used = 1;\n",
        "const unused = 2;\n",
        "</script>\n",
    );
    let Some(found) = check_with(&["noUnusedLocals"], &[Input::Carrier("Usage.vue", usage)]) else {
        return;
    };
    assert_eq!(found, vec![("Usage.vue.ts".to_string(), 6133)]);
}

#[test]
fn generated_scaffolding_stays_live_without_erasing_component_relationships() {
    let generic = r#"<script setup lang="ts" generic="T">
defineProps<{ value: T; format: (value: T) => string }>();
</script>"#;
    let named = r#"<script setup lang="ts">
defineOptions({ name: 'Named' });
defineProps<{ name: string }>();
</script>"#;
    let overloaded = r#"
declare const Component: {
  new (props: { kind: 'number'; value: number }): { readonly $props: { kind: 'number'; value: number } };
  new (props: { kind: 'text'; value: string }): { readonly $props: { kind: 'text'; value: string } };
};
export default Component;
"#;
    let parent = r#"<script setup lang="ts">
import Generic from './Generic.vue';
import Named from './Named.vue';
import Overloaded from './Overloaded';
const used = 1;
const unused = 2;
</script>
<template>
  <Named name="ok" data-extra="ok" />
  <Generic :value="used" :format="value => value.toFixed()" />
  <Generic value="text" :format="value => value.toUpperCase()" />
  <Overloaded kind="number" :value="used" />
  <Overloaded kind="text" value="ok" />
</template>"#;
    let wrong = r#"<script setup lang="ts">
import Named from './Named.vue';
</script><template><Named :name="1" /></template>"#;
    let Some(found) = check_with(
        &["noUnusedLocals", "noUnusedParameters"],
        &[
            Input::Carrier("Generic.vue", generic),
            Input::Carrier("Named.vue", named),
            Input::Module("Overloaded.ts", overloaded),
            Input::Carrier("Parent.vue", parent),
            Input::Carrier("WrongNamed.vue", wrong),
            Input::Carrier("Empty.vue", ""),
            Input::Carrier("Native.vue", "<template><p /></template>"),
        ],
    ) else {
        return;
    };
    assert_eq!(
        found,
        vec![
            ("Parent.vue.ts".to_string(), 6133),
            ("WrongNamed.vue.ts".to_string(), 2322),
        ],
    );
}

#[test]
fn projection_route_let_assigned_later_reads_clean_in_the_template() {
    let camera = concat!(
        "<template>\n  <p>{{ camera.zoom }}</p>\n</template>\n",
        "<script setup lang=\"ts\">\n",
        "import { onMounted } from 'vue'\n",
        "interface Camera { zoom: number }\n",
        "let camera: Camera;\n",
        "onMounted(() => { camera = { zoom: 1 }; });\n",
        "</script>\n",
    );
    let Some(found) = check(&[Input::Carrier("Camera.vue", camera)]) else {
        return;
    };
    assert_eq!(found, Vec::new());
}

#[test]
fn projection_route_shorthand_prop_is_checked_as_its_value() {
    let clean = concat!(
        "<template>\n  <p>{{ t('k', { login }) }}</p>\n</template>\n",
        "<script setup lang=\"ts\">\n",
        "defineProps<{ login: string }>();\n",
        "const t = (key: string, values: { login: string }) => key + values.login;\n",
        "</script>\n",
    );
    let wrong = concat!(
        "<template>\n  <p>{{ t('k', { login }) }}</p>\n</template>\n",
        "<script setup lang=\"ts\">\n",
        "defineProps<{ login: number }>();\n",
        "const t = (key: string, values: { login: string }) => key + values.login;\n",
        "</script>\n",
    );
    let Some(found) = check(&[
        Input::Carrier("Login.vue", clean),
        Input::Carrier("LoginWrong.vue", wrong),
    ]) else {
        return;
    };
    assert_eq!(found, vec![("LoginWrong.vue.ts".to_string(), 2322)]);
}

const NULLABLE_USER: &str = concat!(
    "<script setup lang=\"ts\">\n",
    "const user = null as { id: number; name?: string } | null;\n",
    "const isText = (v: unknown): v is string => typeof v === 'string';\n",
    "const value = 1 as string | number;\n",
    "</script>\n",
);

#[test]
fn template_branches_narrow_like_their_authored_guards() {
    let dominance = format!(
        "{NULLABLE_USER}<template><div v-if=\"user\"><span v-if=\"user.name\">{{{{ user.name.length }}}}</span><span v-else>{{{{ user.id }}}}</span></div></template>\n"
    );
    let false_edge = format!(
        "{NULLABLE_USER}<template><p v-if=\"!user\">none</p><p v-else-if=\"user.id > 1\">{{{{ user.id }}}}</p><p v-else>{{{{ user.name }}}}</p></template>\n"
    );
    let predicate = format!(
        "{NULLABLE_USER}<template><p v-if=\"isText(value)\">{{{{ value.length }}}}</p><p v-else>{{{{ value.toFixed() }}}}</p><i>{{{{ user }}}}</i></template>\n"
    );
    let unguarded = format!(
        "{NULLABLE_USER}<template><p v-if=\"!user\">none</p><p>{{{{ user.id }}}}</p><i>{{{{ isText }}}}{{{{ value }}}}</i></template>\n"
    );
    let Some(found) = check(&[
        Input::Carrier("Dominance.vue", &dominance),
        Input::Carrier("FalseEdge.vue", &false_edge),
        Input::Carrier("Predicate.vue", &predicate),
        Input::Carrier("Unguarded.vue", &unguarded),
    ]) else {
        return;
    };
    assert_eq!(found, vec![("Unguarded.vue.ts".to_string(), 18047)]);
}

#[test]
fn template_branches_minimised_chains_check_clean() {
    let comment_chain = concat!(
        "<script setup lang=\"ts\">\nconst kind = 'a' as string;\n</script>\n",
        "<template>\n  <article>\n    <p v-if=\"kind === 'a'\">a</p>\n",
        "    <!-- A project its red main holds (x): why its starts wait. -->\n",
        "    <p v-else-if=\"kind === 'b'\">b</p>\n    <p v-else>c</p>\n  </article>\n</template>\n",
    );
    let sibling_keys = concat!(
        "<script setup lang=\"ts\">\nimport { ref } from 'vue';\n",
        "const sel = ref<{ id: string } | null>({ id: 'a' });\n</script>\n",
        "<template><div><p v-if=\"sel\" :key=\"sel.id\">a</p>",
        "<span v-if=\"sel\" :key=\"sel.id\">b</span></div></template>\n",
    );
    let Some(found) = check(&[
        Input::Carrier("CommentChain.vue", comment_chain),
        Input::Carrier("SiblingKeys.vue", sibling_keys),
    ]) else {
        return;
    };
    assert_eq!(found, Vec::new());
}

const NAMED_CHILD: &str = concat!(
    "<script setup lang=\"ts\">\n",
    "defineProps<{ name: string }>();\n",
    "</script>\n",
);

fn component_uses_parent(template: &str) -> String {
    format!(
        "<script setup lang=\"ts\">\nimport Child from './Child.vue'\nconst user = null as {{ name: string }} | null;\n</script>\n<template>{template}<i>{{{{ user }}}}</i></template>\n"
    )
}

#[test]
fn component_uses_are_checked_in_their_narrowed_blocks() {
    let narrowed = component_uses_parent("<Child v-if=\"user\" :name=\"user.name\" />");
    let wrong = component_uses_parent("<Child :name=\"1\" />");
    let Some(found) = check(&[
        Input::Carrier("Child.vue", NAMED_CHILD),
        Input::Carrier("Narrowed.vue", &narrowed),
        Input::Carrier("WrongProp.vue", &wrong),
    ]) else {
        return;
    };
    assert_eq!(found, vec![("WrongProp.vue.ts".to_string(), 2322)]);
}

#[test]
fn component_uses_report_a_missing_required_prop() {
    let missing = component_uses_parent("<Child />");
    let Some(found) = check(&[
        Input::Carrier("Child.vue", NAMED_CHILD),
        Input::Carrier("Missing.vue", &missing),
    ]) else {
        return;
    };
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, "Missing.vue.ts");
    // The engine reports the absent member on the construction's argument.
    assert!([2345, 2741, 2769].contains(&found[0].1), "{found:?}");
}

#[test]
fn fallthrough_public_contracts_accept_only_the_fixture_root_surfaces() {
    let fixtures = [
        (
            "ChildDiv.vue",
            include_str!("../tests/cases/fixtures/fallthrough/src/ChildDiv.vue"),
        ),
        (
            "ChildAnchorRoot.vue",
            include_str!("../tests/cases/fixtures/fallthrough/src/ChildAnchorRoot.vue"),
        ),
        (
            "ChildMultiRoot.vue",
            include_str!("../tests/cases/fixtures/fallthrough/src/ChildMultiRoot.vue"),
        ),
        (
            "ChildNoInherit.vue",
            include_str!("../tests/cases/fixtures/fallthrough/src/ChildNoInherit.vue"),
        ),
        (
            "ChildOptionalColliding.vue",
            include_str!("../tests/cases/fixtures/fallthrough/src/ChildOptionalColliding.vue"),
        ),
        (
            "ChildOptionalProp.vue",
            include_str!("../tests/cases/fixtures/fallthrough/src/ChildOptionalProp.vue"),
        ),
        (
            "ChildTyped.vue",
            include_str!("../tests/cases/fixtures/fallthrough/src/ChildTyped.vue"),
        ),
        (
            "WrapsTypedChild.vue",
            include_str!("../tests/cases/fixtures/fallthrough/src/WrapsTypedChild.vue"),
        ),
        (
            "MixedConditionalRoot.vue",
            include_str!("../tests/cases/fixtures/fallthrough/src/MixedConditionalRoot.vue"),
        ),
        (
            "ConditionalRoot.vue",
            include_str!("../tests/cases/fixtures/fallthrough/src/ConditionalRoot.vue"),
        ),
        (
            "OptionsApiAnchorRoot.vue",
            include_str!("../tests/cases/fixtures/fallthrough/src/OptionsApiAnchorRoot.vue"),
        ),
        (
            "ScriptlessAnchorRoot.vue",
            include_str!("../tests/cases/fixtures/fallthrough/src/ScriptlessAnchorRoot.vue"),
        ),
    ];
    let reader = r#"
import BoundChildRoot from './BoundChildRoot.vue';
import NumericWrapper from './NumericWrapper.vue';
import AttrsWrapper from './AttrsWrapper.vue';
import AsyncWrapper from './AsyncWrapper.vue';
import UnionProps from './UnionProps.vue';
import Div from './ChildDiv.vue';
import Anchor from './ChildAnchorRoot.vue';
import Fragment from './ChildMultiRoot.vue';
import Disabled from './ChildNoInherit.vue';
import Numeric from './ChildOptionalColliding.vue';
import Wrapper from './WrapsTypedChild.vue';
import Mixed from './MixedConditionalRoot.vue';
import Conditional from './ConditionalRoot.vue';
import Options from './OptionsApiAnchorRoot.vue';
import Scriptless from './ScriptlessAnchorRoot.vue';
new BoundChildRoot({});
// @ts-expect-error The wrapper already supplies this child's prop.
new BoundChildRoot({ realProp: 42 });
new UnionProps({ mode: 'numeric', title: 42 });
// @ts-expect-error A numeric prop declared in one union arm stays numeric.
new UnionProps({ mode: 'numeric', title: 'wrong' });
new Div({ title: 'native', 'data-test': 1 });
new Anchor({ href: '/target' });
new Numeric({ title: 42 });
new Wrapper({});
new Wrapper({ optProp: 42 });
new NumericWrapper({ value: 42 });
const exposed = new AttrsWrapper<number>({ value: 42, format: n => n.toFixed() });
exposed.value.toFixed();
// @ts-expect-error Root probe metadata stays private.
exposed.__verterRootArm;
// @ts-expect-error Exposed values retain the selected generic argument.
exposed.value.toUpperCase();
new AsyncWrapper({ value: 42, format: n => n.toFixed() });
// @ts-expect-error Authored numeric props retain their declared type.
new NumericWrapper({ value: 'wrong' });
new NumericWrapper({ value: 42, format: (value: number) => value.toFixed() });
// @ts-expect-error The specialized formatter takes a number.
new NumericWrapper({ value: 42, format: (value: string) => value });
new Options({ href: '/options', declaredProp: 42 });
// @ts-expect-error The original Options constructor must not bypass prop types.
new Options({ declaredProp: 'wrong' });
new Scriptless({ href: '/scriptless' });
new Mixed({ pick: true });
new Mixed({ pick: true, title: 42 });
new Mixed({ pick: false, title: 'native' });
new Conditional({ asInput: true, placeholder: 'input branch' });
new Fragment({ class: 'always merged', style: { color: 'red' } });
new Disabled({ class: 'always merged' });
// @ts-expect-error A div has no anchor-only attribute.
new Div({ href: '/wrong' });
// @ts-expect-error Finite root contracts do not gain a string index.
new Anchor({ invented: true });
// @ts-expect-error A fragment has no automatic native attributes.
new Fragment({ title: 'no root' });
// @ts-expect-error Explicitly disabled inheritance adds no native attrs.
new Disabled({ title: 'disabled' });
// @ts-expect-error Declared numeric title takes precedence over HTML title.
new Numeric({ title: 'wrong' });
// @ts-expect-error The child prop remains numeric.
new Wrapper({ optProp: 'wrong' });
// @ts-expect-error The Options default also has a finite root contract.
new Options({ invented: true });
// @ts-expect-error Neither conditional arm accepts this key.
new Mixed({ pick: true, invented: true });
export {};
"#;
    let mut inputs: Vec<_> = fixtures
        .iter()
        .map(|(name, source)| Input::Carrier(name, source))
        .collect();
    inputs.push(Input::Carrier("UnionProps.vue", r#"<script setup lang="ts">defineProps<{ mode: 'numeric'; title: number } | { mode: 'text'; caption: string }>();</script><template><div/></template>"#));
    inputs.push(Input::Carrier("BoundChildRoot.vue", r#"<script setup lang="ts">import Child from './ChildTyped.vue';</script><template><Child :real-prop="42"/></template>"#));
    inputs.push(Input::Carrier("GenericChild.vue", r#"<script setup lang="ts" generic="T">defineProps<{ value: T; format?: (value: T) => string }>();</script><template><div/></template>"#));
    inputs.push(Input::Carrier("NumericWrapper.vue", r#"<script setup lang="ts">import Child from './GenericChild.vue'; defineProps<{ value: number }>();</script><template><Child :value="value"/></template>"#));
    inputs.push(Input::Carrier("AttrsWrapper.vue", r#"<script setup lang="ts" generic="T">import Child from './GenericChild.vue'; import { useAttrs } from 'vue'; const attrs = useAttrs(); void attrs; const props = defineProps<{ value: T }>(); defineExpose({ value: props.value });</script><template><Child :value="value" v-bind="$attrs"/></template>"#));
    inputs.push(Input::Carrier("AsyncWrapper.vue", r#"<script setup lang="ts">import Child from './GenericChild.vue'; defineProps<{ value: number }>(); await Promise.resolve();</script><template><Child :value="value"/></template>"#));
    inputs.push(Input::Module("fallthroughReader.ts", reader));
    let found = check(&inputs).expect("fallthrough requires the real engine and pinned Vue");
    assert_eq!(found, Vec::new());
}

/// A kebab-case attribute on a component reaches the camel-case prop it
/// names, so a correct value is accepted and a wrong one is reported against
/// that prop. An attribute that names neither a prop nor the root element's
/// surface is still rejected under its camel-case spelling, while `data-*`
/// keeps its authored spelling and reaches the root.
#[test]
fn kebab_component_attributes_are_checked_against_the_camel_case_prop_contract() {
    let inputs = [
        Input::Carrier(
            "OptionsApiAdvanced.vue",
            include_str!("../tests/cases/fixtures/diagnostics/src/OptionsApiAdvanced.vue"),
        ),
        Input::Carrier(
            "OptionsApiConsumer.vue",
            include_str!("../tests/cases/fixtures/diagnostics/src/OptionsApiConsumer.vue"),
        ),
        Input::Carrier(
            "ChildDiv.vue",
            include_str!("../tests/cases/fixtures/fallthrough/src/ChildDiv.vue"),
        ),
        Input::Carrier(
            "ChildTyped.vue",
            include_str!("../tests/cases/fixtures/fallthrough/src/ChildTyped.vue"),
        ),
        Input::Carrier(
            "AcceptDataAttr.vue",
            include_str!("../tests/cases/fixtures/fallthrough/src/AcceptDataAttr.vue"),
        ),
        Input::Carrier(
            "RejectUnknownAttr.vue",
            include_str!("../tests/cases/fixtures/fallthrough/src/RejectUnknownAttr.vue"),
        ),
        Input::Carrier(
            "RejectDeclaredPropWrongType.vue",
            include_str!("../tests/cases/fixtures/fallthrough/src/RejectDeclaredPropWrongType.vue"),
        ),
        Input::Carrier(
            "RejectUnknownOnTypedChild.vue",
            include_str!("../tests/cases/fixtures/fallthrough/src/RejectUnknownOnTypedChild.vue"),
        ),
    ];
    let Some(found) = check(&inputs) else {
        return;
    };
    let in_file = |name: &str| {
        found
            .iter()
            .filter(|(file, _)| file == name)
            .map(|(_, code)| *code)
            .collect::<Vec<_>>()
    };
    // `:initial-count="42"` is the declared `initialCount: number`; only the
    // second use, `:initial-count="'wrong'"`, is an error.
    let consumer = in_file("OptionsApiConsumer.vue.ts");
    assert_eq!(consumer.len(), 1, "{found:?}");
    assert!([2322, 2769].contains(&consumer[0]), "{found:?}");
    assert_eq!(in_file("AcceptDataAttr.vue.ts"), Vec::<u32>::new());
    // The wrong value is reported against `realProp` itself, not as a
    // missing `realProp` beside an unknown `real-prop`.
    assert_eq!(in_file("RejectDeclaredPropWrongType.vue.ts"), vec![2322]);
    // `notARealThing` is neither a prop of `ChildDiv` nor a `<div>` attribute.
    assert_eq!(in_file("RejectUnknownAttr.vue.ts"), vec![2345]);
    assert_eq!(in_file("RejectUnknownOnTypedChild.vue.ts"), vec![2345]);
    for child in ["ChildDiv.vue.ts", "ChildTyped.vue.ts"] {
        assert_eq!(in_file(child), Vec::<u32>::new(), "{found:?}");
    }
}

/// An undeclared kebab-case attribute falls through as written (`stroke-width`
/// onto an `<svg>` root) and is not an unknown key, while an alias the surface
/// does not declare (`strokeWidth`) is; `:onCustom-event` satisfies a
/// required `onCustomEvent` prop, also beside a `custom-event` emit; and two
/// keys naming one prop both check against it without a duplicate member.
#[test]
fn kebab_attributes_listener_keys_and_repeated_props_keep_their_contracts() {
    let inputs = [
        Input::Carrier("Icon.vue", "<template><svg/></template>\n"),
        Input::Carrier(
            "IconUse.vue",
            "<script setup lang=\"ts\">import Icon from './Icon.vue'</script>\n<template><Icon stroke-width=\"2\" /></template>\n",
        ),
        Input::Carrier(
            "IconAlias.vue",
            "<script setup lang=\"ts\">import Icon from './Icon.vue'</script>\n<template><Icon stroke-width=\"2\" strokeWidth=\"2\" /></template>\n",
        ),
        Input::Carrier(
            "ListenerProp.vue",
            "<script setup lang=\"ts\">defineProps<{ onCustomEvent: (value: number) => void }>()</script>\n<template><div/></template>\n",
        ),
        Input::Carrier(
            "ListenerPropUse.vue",
            "<script setup lang=\"ts\">import ListenerProp from './ListenerProp.vue'\nconst handle = (value: number) => value\n</script>\n<template><ListenerProp :onCustom-event=\"handle\" /></template>\n",
        ),
        Input::Carrier(
            "FooBar.vue",
            "<script setup lang=\"ts\">defineProps<{ fooBar: number }>()</script>\n<template><div/></template>\n",
        ),
        Input::Carrier(
            "FooBarTwice.vue",
            "<script setup lang=\"ts\">import FooBar from './FooBar.vue'</script>\n<template><FooBar :foo-bar=\"1\" :fooBar=\"2\" /></template>\n",
        ),
        Input::Carrier(
            "FooBarTwiceWrong.vue",
            "<script setup lang=\"ts\">import FooBar from './FooBar.vue'</script>\n<template><FooBar :foo-bar=\"1\" :fooBar=\"'x'\" /></template>\n",
        ),
        Input::Carrier(
            "PropAndEmit.vue",
            "<script setup lang=\"ts\">defineProps<{ onCustomEvent: (value: number) => void }>()\ndefineEmits<{ 'custom-event': [value: number] }>()</script>\n<template><div/></template>\n",
        ),
        Input::Carrier(
            "PropAndEmitUse.vue",
            "<script setup lang=\"ts\">import PropAndEmit from './PropAndEmit.vue'\nconst handle = (value: number) => value\n</script>\n<template><PropAndEmit :onCustom-event=\"handle\" /></template>\n",
        ),
    ];
    let Some(found) = check(&inputs) else {
        return;
    };
    let in_file = |name: &str| {
        found
            .iter()
            .filter(|(file, _)| file == name)
            .map(|(_, code)| *code)
            .collect::<Vec<_>>()
    };
    for clean in [
        "Icon.vue.ts",
        "IconUse.vue.ts",
        "FooBar.vue.ts",
        "FooBarTwice.vue.ts",
        "ListenerProp.vue.ts",
        "ListenerPropUse.vue.ts",
        "PropAndEmit.vue.ts",
        "PropAndEmitUse.vue.ts",
    ] {
        assert_eq!(in_file(clean), Vec::<u32>::new(), "{clean}: {found:?}");
    }
    // `strokeWidth` is neither a prop nor an `<svg>` attribute.
    assert_eq!(in_file("IconAlias.vue.ts"), vec![2345], "{found:?}");
    assert_eq!(in_file("FooBarTwiceWrong.vue.ts"), vec![2322], "{found:?}");
}
