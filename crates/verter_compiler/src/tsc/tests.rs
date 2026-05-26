use super::script::{generate_tsc_output_with_options, TscGenOptions, TscMode};
use crate::utils::oxc::vue::resolve_type::resolve_external_type;
use oxc_allocator::Allocator;
use oxc_sourcemap::SourceMap;
use rustc_hash::FxHashMap;

fn gen_tsc(sfc: &str) -> String {
    gen_tsc_output(sfc).code
}

fn gen_tsc_output(sfc: &str) -> super::script::TscOutput {
    generate_tsc_output_with_options(
        sfc,
        "TestComp",
        &TscGenOptions {
            filename: Some("/test/TestComp.vue".to_string()),
            ..Default::default()
        },
    )
}

fn gen_tsc_with_external_type(sfc: &str, type_name: &str, dep_source: &str) -> String {
    gen_tsc_output_with_external_type(sfc, type_name, dep_source).code
}

fn gen_tsc_output_with_external_type(
    sfc: &str,
    type_name: &str,
    dep_source: &str,
) -> super::script::TscOutput {
    gen_tsc_output_with_external_type_and_mode(sfc, type_name, dep_source, TscMode::Public)
}

fn gen_tsc_output_with_external_type_and_mode(
    sfc: &str,
    type_name: &str,
    dep_source: &str,
    mode: TscMode,
) -> super::script::TscOutput {
    let alloc = Allocator::default();
    let resolved = resolve_external_type(type_name, dep_source, &alloc)
        .expect("failed to resolve external type");
    let mut external_types = FxHashMap::default();
    external_types.insert(type_name.to_string(), resolved);
    generate_tsc_output_with_options(
        sfc,
        "TestComp",
        &TscGenOptions {
            filename: Some("/test/TestComp.vue".to_string()),
            external_types: Some(external_types),
            mode,
            ..Default::default()
        },
    )
}

fn gen_tsc_narrowing(sfc: &str) -> String {
    generate_tsc_output_with_options(
        sfc,
        "TestComp",
        &TscGenOptions {
            conditional_root_narrowing: true,
            ..Default::default()
        },
    )
    .code
}

fn gen_tsc_testing(sfc: &str) -> String {
    gen_tsc_output_testing(sfc).code
}

fn gen_tsc_output_testing(sfc: &str) -> super::script::TscOutput {
    generate_tsc_output_with_options(
        sfc,
        "TestComp",
        &TscGenOptions {
            filename: Some("/test/TestComp.vue".to_string()),
            mode: TscMode::Testing,
            ..Default::default()
        },
    )
}

fn gen_tsc_testing_with_external_type(sfc: &str, type_name: &str, dep_source: &str) -> String {
    gen_tsc_output_with_external_type_and_mode(sfc, type_name, dep_source, TscMode::Testing).code
}

fn offset_to_zero_based_line_col(text: &str, offset: usize) -> (u32, u32) {
    let mut line = 0u32;
    let mut col = 0u32;
    for ch in text[..offset].chars() {
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

// ── defineProps<ImportedType>() — type-only, no runtime props ────────────────

#[test]
fn tsc_codegen_type_only_props_inlined_in_declare() {
    let r = gen_tsc(
        r#"<script setup>
import type { Props } from './types'
defineProps<Props>()
</script><template><div>hello</div></template>"#,
    );

    assert!(
        r.contains("import type { Props } from './types'"),
        "type import statement emitted"
    );
    assert!(r.contains("defineComponent("), "defineComponent present");
    assert!(!r.contains("props: {"), "no runtime props for type-only");
    assert!(
        r.contains("$props: import(\"vue\").PublicProps & Props"),
        "type name in new()"
    );
    assert!(r.contains("PublicProps"), "PublicProps in constructor");
    assert!(
        !r.contains("import('./types').Props"),
        "should not use inline import() syntax"
    );
    assert!(r.contains("export default"), "export default present");
    assert!(r.contains("sourceMappingURL"), "source map present");
    assert!(!r.contains("___VERTER___"), "no IDE wrapper");
    assert!(!r.contains("setup("), "no setup() in __comp");
    assert!(!r.contains("__verter_"), "no intermediate aliases");
}

// @ai-generated - Testing mode exposes internal script-setup bindings on the instance.
#[test]
fn tsc_testing_mode_exposes_local_script_setup_bindings_on_instance() {
    let public = gen_tsc(
        r#"<script setup lang="ts">
const count = 1
const label = 'hello'
</script><template><div>{{ count }} {{ label }}</div></template>"#,
    );
    let testing = gen_tsc_testing(
        r#"<script setup lang="ts">
const count = 1
const label = 'hello'
</script><template><div>{{ count }} {{ label }}</div></template>"#,
    );

    assert!(
        testing.contains("type __Verter_TestBindings = import(\"vue\").ShallowUnwrapRef<{"),
        "testing mode should emit a debug binding helper: {testing}"
    );
    assert!(
        testing.contains("count: typeof count"),
        "testing mode should expose count on the instance: {testing}"
    );
    assert!(
        testing.contains("label: typeof label"),
        "testing mode should expose label on the instance: {testing}"
    );
    assert!(
        !testing.contains("ref: typeof ref"),
        "value imports must not become instance bindings: {testing}"
    );
    assert!(
        !public.contains("count: typeof count"),
        "public mode must keep script-setup bindings hidden: {public}"
    );
}

// @ai-generated - Testing mode mirrors VTU wrapper.vm shallow ref unwrapping.
#[test]
fn tsc_testing_mode_unwraps_ref_like_bindings_on_instance() {
    let testing = gen_tsc_testing(
        r#"<script setup lang="ts">
import { computed, ref } from 'vue'

const count = ref(1)
const doubled = computed(() => count.value * 2)
</script><template><div>{{ doubled }}</div></template>"#,
    );

    assert!(
        testing.contains("type __Verter_TestBindings = import(\"vue\").ShallowUnwrapRef<{"),
        "testing mode should use ShallowUnwrapRef for instance bindings: {testing}"
    );
    assert!(
        testing.contains("count: typeof count"),
        "ref bindings should be included before unwrapping: {testing}"
    );
    assert!(
        testing.contains("doubled: typeof doubled"),
        "computed bindings should be included before unwrapping: {testing}"
    );
    assert!(
        !testing.contains("ref: typeof ref"),
        "imported helpers must stay out of the instance binding map: {testing}"
    );
}

// @ai-generated - defineExpose must not narrow test-only wrapper.vm bindings.
#[test]
fn tsc_testing_mode_ignores_define_expose_narrowing() {
    let testing = gen_tsc_testing(
        r#"<script setup lang="ts">
import { ref } from 'vue'

const foo = ref(1)
const bar = ref('hidden')

defineExpose({ foo })
</script><template><div>{{ foo }}</div></template>"#,
    );

    assert!(
        testing.contains("foo: typeof foo"),
        "explicitly exposed bindings should still be present: {testing}"
    );
    assert!(
        testing.contains("bar: typeof bar"),
        "non-exposed bindings must remain available in testing mode: {testing}"
    );
    assert!(
        !testing.contains("defineExpose({ foo }) as"),
        "testing mode should not rewrite defineExpose into a narrowing helper: {testing}"
    );
}

// ── defineProps({ ... }) — object syntax, runtime + TS types ─────────────────

#[test]
fn tsc_codegen_props_object_syntax_runtime_and_typed() {
    let r = gen_tsc(
        r#"<script setup>
defineProps({ title: String, count: { type: Number, required: true } })
</script><template/>"#,
    );

    assert!(r.contains("props: {"), "runtime props in __comp");
    assert!(r.contains("title: String"), "runtime String constructor");
    assert!(
        r.contains("{ type: Number, required: true }"),
        "runtime Number required"
    );
    assert!(r.contains("title?: string"), "optional string in declare");
    assert!(r.contains("count: number"), "required number in declare");
    assert!(!r.contains("defineProps"), "macro removed");
    assert!(!r.contains("setup("), "no setup() in __comp");
}

// ── defineModel<string>('name') — runtime props + emits + TS types ───────────

#[test]
fn tsc_codegen_define_model_runtime_and_typed() {
    let r = gen_tsc(
        r#"<script setup>
const title = defineModel<string>('title')
</script><template/>"#,
    );

    assert!(r.contains("props: {"), "runtime props in __comp");
    assert!(r.contains("title: String"), "runtime model prop");
    assert!(r.contains("emits: ["), "runtime emits in __comp");
    assert!(r.contains("'update:title'"), "runtime model emit");
    assert!(
        r.contains("\"onUpdate:title\""),
        "model onUpdate prop in declare"
    );
    assert!(
        r.contains("event: 'update:title', v: string"),
        "model emit type in $emit overload"
    );
    assert!(!r.contains("defineModel"), "macro removed");
    assert!(!r.contains("__verter_"), "no intermediate aliases");
    assert!(!r.contains("const title"), "no script body variable");
    assert!(!r.contains("setup("), "no setup() in __comp");
}

// ── defineEmits(['...']) — array syntax runtime + typed ──────────────────────

#[test]
fn tsc_codegen_emits_array_syntax() {
    let r = gen_tsc(
        r#"<script setup>
defineEmits(['change', 'update:model'])
</script><template/>"#,
    );

    assert!(r.contains("emits: ["), "runtime emits in __comp");
    assert!(r.contains("'change'"), "runtime emits has change");
    assert!(
        r.contains("'update:model'"),
        "runtime emits has update:model"
    );
    assert!(
        r.contains("event: 'update:model'"),
        "typed emits in $emit overload"
    );
    assert!(!r.contains("setup("), "no setup() in __comp");
}

// ── defineOptions({ name, inheritAttrs }) — options in __comp ────────────────

#[test]
fn tsc_codegen_define_options_in_comp() {
    let r = gen_tsc(
        r#"<script setup>
defineOptions({ name: 'MyComp', inheritAttrs: false })
</script><template/>"#,
    );

    assert!(
        r.contains("name: 'MyComp' as const"),
        "name as const in __comp"
    );
    assert!(r.contains("inheritAttrs: false"), "inheritAttrs in __comp");
    assert!(!r.contains("setup("), "no setup() in __comp");
}

// ── Script body + value imports must not appear in output ────────────────────

#[test]
fn tsc_codegen_no_body_no_value_imports() {
    let r = gen_tsc(
        r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
const doubled = computed(() => count.value * 2)
</script><template/>"#,
    );

    assert!(!r.contains("const count"), "no ref variable in output");
    assert!(
        !r.contains("const doubled"),
        "no computed variable in output"
    );
    assert!(!r.contains("import { ref }"), "value import not in output");
    assert!(!r.contains("setup("), "no setup() in __comp");
}

// ── SFC without <script setup> returns a stub ─────────────────────────────────

#[test]
fn tsc_codegen_no_script_setup_returns_stub() {
    let r = gen_tsc(r#"<template><div>hello</div></template>"#);

    assert!(r.contains("defineComponent"), "stub has defineComponent");
    assert!(r.contains("export default"), "stub has export default");
    assert!(r.contains("sourceMappingURL"), "stub has sourceMappingURL");
}

// ── Options API stub preserves defineComponent props ───────────────────────

#[test]
fn tsc_codegen_options_api_preserves_props() {
    let r = gen_tsc(
        r#"<script lang="ts">
import { defineComponent } from 'vue'
export default defineComponent({
  props: {
    count: { type: Number, required: true },
    label: String
  }
})
</script>
<template><div>{{ count }}</div></template>"#,
    );

    assert!(r.contains("defineComponent"), "stub has defineComponent");
    assert!(r.contains("export default"), "stub has export default");
    // The stub must preserve the actual props so cross-component type checking works
    assert!(
        r.contains("count"),
        "stub must preserve prop 'count' for cross-component type checking:\n{r}"
    );
    assert!(
        r.contains("Number"),
        "stub must preserve prop type 'Number':\n{r}"
    );
    assert!(
        !r.contains("defineComponent({})"),
        "stub must NOT be the empty defineComponent({{}}) placeholder:\n{r}"
    );
}

// ── Options API plain object wrapping with defineComponent ─────────────────

#[test]
fn tsc_options_api_plain_object_gets_define_component_wrap() {
    let r = gen_tsc(
        r#"<script>
export default {
  data() { return { count: 0 } },
  methods: { increment() { this.count++ } }
}
</script>
<template><div>{{ count }}</div></template>"#,
    );

    // Positive: should have defineComponent wrapping
    assert!(
        r.contains("defineComponent("),
        "plain object should be wrapped with defineComponent:\n{r}"
    );
    assert!(
        r.contains("import { defineComponent }"),
        "should add defineComponent import:\n{r}"
    );
    // Positive: original content preserved inside the wrap
    assert!(
        r.contains("data()"),
        "data() must be preserved inside defineComponent wrap:\n{r}"
    );

    // Negative: should not have bare object as default export
    // (the object literal should be inside defineComponent())
    assert!(
        !r.contains("export default {"),
        "plain object should not remain bare — must be wrapped with defineComponent:\n{r}"
    );
}

#[test]
fn tsc_options_api_with_define_component_not_double_wrapped() {
    let r = gen_tsc(
        r#"<script lang="ts">
import { defineComponent } from 'vue'
export default defineComponent({
  data() { return { count: 0 } }
})
</script>
<template><div>{{ count }}</div></template>"#,
    );

    // Positive: defineComponent preserved
    assert!(
        r.contains("defineComponent("),
        "existing defineComponent should be preserved:\n{r}"
    );

    // Negative: should not double-wrap
    let count = r.matches("defineComponent(").count();
    assert_eq!(
        count, 1,
        "should not double-wrap defineComponent, got {count} occurrences in:\n{r}"
    );
}

#[test]
fn tsc_options_api_non_object_export_not_wrapped() {
    let r = gen_tsc(
        r#"<script>
const MyComponent = { data() { return {} } }
export default MyComponent
</script>
<template><div></div></template>"#,
    );

    // Negative: identifier export should NOT get defineComponent wrap
    assert!(
        !r.contains("import { defineComponent }"),
        "identifier export should not get defineComponent import added:\n{r}"
    );
}

// ── PropType<X> extraction ──────────────────────────────────────────────────

#[test]
fn tsc_codegen_proptype_extraction() {
    let r = gen_tsc(
        r#"<script setup>
import type { PropType } from 'vue'
defineProps({
  items: Array as PropType<string[]>,
  handler: Function as PropType<(e: Event) => void>
})
</script><template/>"#,
    );

    assert!(r.contains("items?: string[]"), "PropType<string[]>");
    assert!(
        r.contains("handler?: (e: Event) => void"),
        "PropType function"
    );
    let comp_section = r.split("declare const").next().unwrap();
    assert!(
        !comp_section.contains("as PropType"),
        "no PropType cast in runtime"
    );
    assert!(!r.contains("setup("), "no setup() in __comp");
}

// ── Factory function default ──────────────────────────────────────────────────

#[test]
fn tsc_codegen_factory_function_default() {
    let r = gen_tsc(
        r#"<script setup>
import type { PropType } from 'vue'
defineProps({
  config: { type: Object as PropType<{name: string}>, default: () => ({}) }
})
</script><template/>"#,
    );

    assert!(
        r.contains("config?: {name: string}"),
        "PropType in object form: got {}",
        r
    );
    assert!(!r.contains("setup("), "no setup() in __comp");
}

// ── Mixed type sources ──────────────────────────────────────────────────────

#[test]
fn tsc_codegen_mixed_type_sources() {
    let r = gen_tsc(
        r#"<script setup>
import type { PropType } from 'vue'
defineProps({
  title: String,
  count: { type: Number, required: true },
  items: Array as PropType<string[]>,
  config: { type: Object as PropType<{name: string}>, required: true }
})
</script><template/>"#,
    );

    assert!(r.contains("title?: string"), "simple constructor");
    assert!(r.contains("count: number"), "required number");
    assert!(r.contains("items?: string[]"), "PropType annotation");
    assert!(
        r.contains("config: {name: string}"),
        "PropType in object form"
    );
    assert!(!r.contains("setup("), "no setup() in __comp");
}

// ── Complex real-world SFC ──────────────────────────────────────────────────

#[test]
fn tsc_codegen_complex_real_world_sfc() {
    let r = gen_tsc(
        r#"<script setup>
import { ref, computed } from 'vue'
import type { PropType } from 'vue'

defineOptions({ name: 'MyForm', inheritAttrs: false })

const emit = defineEmits(['submit', 'cancel'])

const props = defineProps({
  title: String,
  maxLength: { type: Number, required: true },
  items: Array as PropType<string[]>,
  config: { type: Object as PropType<{name: string}>, default: () => ({}) }
})

const name = defineModel<string>('name')

const inputRef = ref(null)
const isValid = computed(() => props.title !== '')
</script>
<template>
  <form @submit.prevent="emit('submit')">
    <input v-model="name" :ref="inputRef" />
  </form>
</template>"#,
    );

    assert!(!r.contains("const inputRef"), "no ref variable");
    assert!(!r.contains("const isValid"), "no computed variable");
    assert!(!r.contains("const props"), "no props variable");
    assert!(!r.contains("const emit"), "no emit variable");
    assert!(!r.contains("import { ref"), "no value imports");
    assert!(!r.contains("setup("), "no setup()");

    let comp_section = r.split("declare const").next().unwrap();
    assert!(
        comp_section.contains("name: 'MyForm' as const"),
        "name option"
    );
    assert!(comp_section.contains("inheritAttrs: false"), "inheritAttrs");
    assert!(comp_section.contains("props: {"), "runtime props");
    assert!(comp_section.contains("title: String"), "runtime String");
    assert!(
        !comp_section.contains("as PropType"),
        "no PropType in runtime"
    );
    assert!(comp_section.contains("emits: ["), "runtime emits");
    assert!(comp_section.contains("'submit'"), "submit emit");
    assert!(comp_section.contains("'cancel'"), "cancel emit");
    assert!(comp_section.contains("'update:name'"), "model emit");

    let declare_section = r.split("declare const").nth(1).unwrap();
    assert!(
        declare_section.contains("title?: string"),
        "optional string"
    );
    assert!(
        declare_section.contains("maxLength: number"),
        "required number"
    );
    assert!(
        declare_section.contains("items?: string[]"),
        "PropType annotation"
    );
    assert!(
        declare_section.contains("config?: {name: string}"),
        "PropType with default"
    );
}

// ── Runtime stripping: as PropType<X> removed from __comp ───────────────────

#[test]
fn tsc_codegen_runtime_stripping_as_proptype() {
    let r = gen_tsc(
        r#"<script setup>
import type { PropType } from 'vue'
defineProps({ items: Array as PropType<string[]> })
</script><template/>"#,
    );

    let comp_section = r.split("declare const").next().unwrap();
    assert!(
        !comp_section.contains("as PropType"),
        "no PropType cast in runtime section"
    );
    assert!(comp_section.contains("items: Array"), "constructor kept");
}

// ── withDefaults makes props optional ────────────────────────────────────────

#[test]
fn tsc_codegen_with_defaults_makes_props_optional() {
    let r = gen_tsc(
        r#"<script setup>
withDefaults(defineProps<{ title: string; count: number }>(), {
  title: 'hello'
})
</script><template/>"#,
    );

    assert!(r.contains("title?: string"), "title optional with default");
    assert!(
        r.contains("count: number"),
        "count required without default"
    );
    assert!(!r.contains("withDefaults"), "macro removed");
    assert!(!r.contains("setup("), "no setup()");
}

// ── withDefaults with imported type ──────────────────────────────────────────

#[test]
fn tsc_codegen_with_defaults_imported_type() {
    let r = gen_tsc(
        r#"<script setup>
import type { Props } from './types'
withDefaults(defineProps<Props>(), { title: 'hello' })
</script><template/>"#,
    );

    assert!(
        r.contains("import type { Props } from './types'"),
        "import type statement present"
    );
    assert!(
        r.contains("Omit<Props, 'title'> & Partial<Pick<Props, 'title'>>"),
        "defaulted imported props should be wrapped to make keys optional: {r}"
    );
    assert!(!r.contains("withDefaults"), "macro removed");
}

// @ai-generated - Imported prop types must not slice foreign spans when defaults are present.
#[test]
fn tsc_codegen_public_mode_with_defaults_imported_type_does_not_panic() {
    let r = gen_tsc_with_external_type(
        r#"<script setup lang="ts">
import type { Props } from './types'
withDefaults(defineProps<Props>(), {
  title: 'hello',
})
</script><template><div>{{ title }} {{ count }}</div></template>"#,
        "Props",
        r#"
export interface Props {
  title: string
  count: number
}
"#,
    );

    assert!(
        r.contains("import type { Props } from './types'"),
        "import type statement should be preserved: {r}"
    );
    assert!(
        r.contains("Omit<Props, 'title'> & Partial<Pick<Props, 'title'>>"),
        "defaulted imported props should stay wrapped as a named type: {r}"
    );
}

// @ai-generated - Testing mode should expose imported props without indexing into foreign source text.
#[test]
fn tsc_testing_mode_with_defaults_imported_type_uses_indexed_access_without_panicking() {
    let r = gen_tsc_testing_with_external_type(
        r#"<script setup lang="ts">
import type { Props } from './types'
withDefaults(defineProps<Props>(), {
  title: 'hello',
})
</script><template><div>{{ title }} {{ count }}</div></template>"#,
        "Props",
        r#"
export interface Props {
  title: string
  count: number
}
"#,
    );

    assert!(
        r.contains("declare const title: Props['title']"),
        "defaulted imported props should use indexed access in testing mode: {r}"
    );
    assert!(
        r.contains("declare const count: Props['count']"),
        "non-defaulted imported props should use indexed access in testing mode: {r}"
    );
}

// @ai-generated - Companion-script resolved prop spans should be consumed as absolute SFC spans.
#[test]
fn tsc_testing_mode_same_sfc_companion_props_use_absolute_spans() {
    let r = gen_tsc_testing(
        r#"<script lang="ts">
export interface Props {
  title: string
  count?: number
}
</script>
<script setup lang="ts">
withDefaults(defineProps<Props>(), {
  title: 'hello',
})
</script><template><div>{{ title }} {{ count }}</div></template>"#,
    );

    assert!(
        r.contains("declare const title: string"),
        "defaulted companion-script props should keep their concrete type text: {r}"
    );
    assert!(
        r.contains("declare const count: (number) | undefined"),
        "optional companion-script props should still render optional types: {r}"
    );
}

// ── Object prop with default is optional ─────────────────────────────────────

#[test]
fn tsc_codegen_object_prop_with_default_is_optional() {
    let r = gen_tsc(
        r#"<script setup>
defineProps({ color: { type: String, default: 'red' } })
</script><template/>"#,
    );

    assert!(r.contains("color?: string"), "default makes it optional");
}

// ── PropType with default is optional ────────────────────────────────────────

#[test]
fn tsc_codegen_proptype_with_default_is_optional() {
    let r = gen_tsc(
        r#"<script setup>
import type { PropType } from 'vue'
defineProps({ config: { type: Object as PropType<{name: string}>, default: () => ({}) } })
</script><template/>"#,
    );

    assert!(
        r.contains("config?: {name: string}"),
        "PropType with default is optional"
    );
}

// ── Union type array: [String, Number, Boolean] ─────────────────────────────

#[test]
fn tsc_codegen_union_type_array_prop() {
    let r = gen_tsc(
        r#"<script setup>
defineProps({
  value: [String, Number, Boolean],
  mixed: { type: [String, Number], required: true }
})
</script><template/>"#,
    );

    assert!(
        r.contains("value?: string | number | boolean"),
        "union type from array"
    );
    assert!(
        r.contains("mixed: string | number"),
        "union type from object array"
    );
    // Verify the $props type has no unknown — the emits fallback may contain unknown
    let props_section = r
        .split("$props:")
        .nth(1)
        .and_then(|s| s.split(',').next())
        .unwrap_or("");
    assert!(
        !props_section.contains("unknown"),
        "no unknown in props types"
    );
    assert!(!r.contains("setup("), "no setup()");
}

// ── defineModel default name (no arg) ────────────────────────────────────────

#[test]
fn tsc_codegen_define_model_default_name() {
    let r = gen_tsc(
        r#"<script setup>
const mv = defineModel<number>()
</script><template/>"#,
    );

    assert!(r.contains("modelValue: Number"), "runtime modelValue prop");
    assert!(
        r.contains("'update:modelValue'"),
        "runtime update:modelValue emit"
    );
    assert!(
        r.contains("modelValue?: number"),
        "TS modelValue type in $props"
    );
    assert!(r.contains("\"onUpdate:modelValue\""), "TS onUpdate handler");
    assert!(!r.contains("const mv"), "no script body variable");
}

// ── Multiple defineModel calls ───────────────────────────────────────────────

#[test]
fn tsc_codegen_multiple_define_models() {
    let r = gen_tsc(
        r#"<script setup>
const first = defineModel<string>('firstName')
const last = defineModel<string>('lastName')
</script><template/>"#,
    );

    assert!(r.contains("firstName: String"), "runtime firstName prop");
    assert!(r.contains("lastName: String"), "runtime lastName prop");
    assert!(r.contains("'update:firstName'"), "runtime update:firstName");
    assert!(r.contains("'update:lastName'"), "runtime update:lastName");
    assert!(r.contains("firstName?: string"), "TS firstName type");
    assert!(r.contains("lastName?: string"), "TS lastName type");
}

// ── defineModel with no type parameter ───────────────────────────────────────

#[test]
fn tsc_codegen_define_model_no_type() {
    let r = gen_tsc(
        r#"<script setup>
const val = defineModel('value')
</script><template/>"#,
    );

    assert!(r.contains("value: Object"), "runtime prop with Object ctor");
    assert!(
        r.contains("value?: unknown"),
        "TS type is unknown without type param"
    );
}

// ── defineModel with imported type ──────────────────────────────────────────

#[test]
fn tsc_codegen_define_model_imported_type() {
    let r = gen_tsc(
        r#"<script setup>
import type { User } from './types'
const user = defineModel<User>()
</script><template/>"#,
    );

    assert!(
        r.contains("import type { User } from './types'"),
        "import type statement for model should be emitted: {r}"
    );
    assert!(r.contains("modelValue?: User"), "TS User type");
    assert!(
        r.contains("event: 'update:modelValue', v: User"),
        "emit type with User in $emit overload"
    );
}

#[test]
fn tsc_codegen_define_model_local_type_dependencies_are_emitted() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
interface Role {
  name: string
}

interface User {
  role: Role
}

const user = defineModel<User>()
</script><template/>"#,
    );

    assert!(
        r.contains("interface User"),
        "User interface should be emitted: {r}"
    );
    assert!(
        r.contains("interface Role"),
        "Role interface should be emitted: {r}"
    );
    assert!(
        r.contains("modelValue?: User"),
        "model should keep the named User type: {r}"
    );
}

// ── defineModel combined with defineProps ────────────────────────────────────

#[test]
fn tsc_codegen_define_model_with_define_props() {
    let r = gen_tsc(
        r#"<script setup>
defineProps({ label: String })
const text = defineModel<string>()
</script><template/>"#,
    );

    assert!(r.contains("label: String"), "runtime label prop");
    assert!(
        r.contains("modelValue: String"),
        "runtime modelValue from model"
    );
    assert!(r.contains("label?: string"), "TS label type");
    assert!(r.contains("modelValue?: string"), "TS modelValue type");
}

// ── Edge cases ──────────────────────────────────────────────────────────────

#[test]
fn tsc_codegen_edge_cases() {
    let r = gen_tsc(
        r#"<script setup>
import type { PropType } from 'vue'
defineProps({
  cb: Function,
  obj: Object,
  arr: Array,
  sym: Symbol,
  disabled: Boolean,
  items: { type: Array as PropType<string[]>, required: true },
})
</script><template/>"#,
    );

    assert!(
        r.contains("cb?: (...args: unknown[]) => unknown"),
        "Function type"
    );
    assert!(r.contains("obj?: Record<string, unknown>"), "Object type");
    assert!(r.contains("arr?: unknown[]"), "Array type");
    assert!(r.contains("sym?: symbol"), "Symbol type");
    assert!(r.contains("disabled?: boolean"), "Boolean type");
    assert!(r.contains("items: string[]"), "required PropType");
    assert!(!r.contains("setup("), "no setup()");
}

// ── Print output for real-world SFCs ─────────────────────────────────────────

#[test]
fn tsc_codegen_print_real_world_coreui() {
    let r = gen_tsc(
        r##"<script setup>
const props = defineProps({
  href: String,
  tabContentClass: String,
})

const url = `https://coreui.io/vue/docs/${props.href}`
const addClass = props.tabContentClass
</script>
<template>
  <div class="example">
    <CNav variant="underline-border">
      <CNavItem>
        <CNavLink href="#" active>
          <CIcon icon="cil-media-play" class="me-2" />
          Preview
        </CNavLink>
      </CNavItem>
    </CNav>
  </div>
</template>"##,
    );
    eprintln!("\n=== CoreUI DocsExample.vue ===\n{}\n", r);

    assert!(r.contains("href?: string"), "href prop");
    assert!(
        r.contains("tabContentClass?: string"),
        "tabContentClass prop"
    );
    assert!(!r.contains("const url"), "no script body");
    assert!(!r.contains("const addClass"), "no script body");
}

#[test]
fn tsc_codegen_print_real_world_slidev() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineProps<{
  disabled?: boolean
}>()

const value = defineModel<boolean>('modelValue', {
  type: Boolean,
})
</script>
<template>
  <div border="~ main rounded">
    <div i-ri-check-line :class="value ? '' : 'op0'" />
    <input v-model="value" type="checkbox" :disabled="disabled">
  </div>
</template>"#,
    );
    eprintln!("\n=== Slidev FormCheckbox.vue ===\n{}\n", r);

    assert!(r.contains("disabled?: boolean"), "disabled prop");
    assert!(r.contains("modelValue"), "modelValue from defineModel");
    assert!(!r.contains("const value"), "no script body");
}

#[test]
fn tsc_codegen_print_real_world_element_plus_watermark() {
    let r = gen_tsc(
        r#"<script lang="ts" setup>
import { computed, onBeforeUnmount, onMounted, ref, shallowRef, watch } from 'vue'
import { useMutationObserver } from '@vueuse/core'
import { isArray, isUndefined } from '@element-plus/utils'
import { getPixelRatio, getStyleStr, reRendering } from './utils'
import useClips from './useClips'

import type { WatermarkProps } from './watermark'
import type { CSSProperties } from 'vue'

defineOptions({
  name: 'ElWatermark',
})

const style: CSSProperties = {
  position: 'relative',
}

const props = withDefaults(defineProps<WatermarkProps>(), {
  zIndex: 9,
  rotate: -22,
  content: 'Element Plus',
  gap: () => [100, 100],
})
const fontGap = computed(() => props.font?.fontGap ?? 3)
const color = computed(() => props.font?.color ?? 'rgba(0,0,0,.15)')
const containerRef = shallowRef<HTMLDivElement | null>(null)
const watermarkRef = shallowRef<HTMLDivElement>()
const stopObservation = ref(false)
</script>
<template>
  <div ref="containerRef" :style="[style]">
    <slot />
  </div>
</template>"#,
    );
    eprintln!("\n=== Element Plus watermark.vue ===\n{}\n", r);

    assert!(r.contains("name: 'ElWatermark' as const"), "name option");
    assert!(
        r.contains("import type { WatermarkProps } from './watermark'"),
        "imported type as import statement"
    );
    assert!(
        r.contains(
            "$props: import(\"vue\").PublicProps & Omit<WatermarkProps, 'zIndex' | 'rotate' | 'content' | 'gap'> & Partial<Pick<WatermarkProps, 'zIndex' | 'rotate' | 'content' | 'gap'>>"
        ),
        "defaulted imported props should be optional in $props"
    );
    assert!(!r.contains("const style"), "no script body");
    assert!(!r.contains("const fontGap"), "no computed");
    assert!(!r.contains("import { computed"), "no value imports");
}

#[test]
fn tsc_codegen_print_real_world_complex_type_syntax() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import type { HTMLAttributes } from 'vue'

interface CarouselProps {
  opts?: Record<string, unknown>
  plugins?: unknown[]
  orientation?: 'horizontal' | 'vertical'
  class?: HTMLAttributes['class']
}

const props = withDefaults(defineProps<CarouselProps>(), {
  orientation: 'horizontal',
})

const emits = defineEmits<{
  (e: 'init-api', api: unknown): void
}>()

const dir = ref<'ltr' | 'rtl'>('ltr')
</script>
<template>
  <div :class="props.class" :dir="dir">
    <slot />
  </div>
</template>"#,
    );
    eprintln!(
        "\n=== Carousel.vue (type syntax + withDefaults + emits) ===\n{}\n",
        r
    );

    assert!(
        r.contains("orientation?"),
        "orientation optional via default"
    );
    assert!(r.contains("'init-api'"), "emit name in output");
    assert!(!r.contains("const dir"), "no script body");
}

// ── Union runtime function type must be parenthesized (TS1385) ───────────────

#[test]
fn tsc_codegen_union_runtime_function() {
    let r = gen_tsc(
        r#"<script setup>
defineProps({ msg: [String, Function] })
</script><template/>"#,
    );

    assert!(
        r.contains("msg?: string | ((...args: unknown[]) => unknown)"),
        "Function in union must be parenthesized: got {}",
        r
    );
    // Negative: must NOT have unparenthesized arrow in a union
    assert!(
        !r.contains("string | (...args"),
        "unparenthesized function type in union"
    );
}

#[test]
fn tsc_codegen_single_runtime_function() {
    let r = gen_tsc(
        r#"<script setup>
defineProps({ cb: Function })
</script><template/>"#,
    );

    assert!(
        r.contains("cb?: (...args: unknown[]) => unknown"),
        "single Function: no extra parens needed: got {}",
        r
    );
    // Negative: must NOT have outer parens when not in a union
    assert!(
        !r.contains("cb?: ((...args"),
        "single function should not have outer parens"
    );
}

#[test]
fn tsc_codegen_proptype_union_parens() {
    let r = gen_tsc(
        r#"<script setup>
import type { PropType } from 'vue'
defineProps({ msg: String as PropType<string | (() => any)> })
</script><template/>"#,
    );

    assert!(
        r.contains("msg?: string | (() => any)"),
        "PropType union with parenthesized function preserved: got {}",
        r
    );
}

#[test]
fn tsc_codegen_type_only_union_function() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineProps<{ cb: string | (() => void) }>()
</script><template/>"#,
    );

    assert!(
        r.contains("cb: string | (() => void)"),
        "type-only union function preserved from source: got {}",
        r
    );
    // Negative: must NOT lose the parens around the arrow function
    assert!(
        !r.contains("string | () =>"),
        "must not lose parens around arrow function type"
    );
}

// ── Nested object prop with PropType inside — no truncation ──────────────────

#[test]
fn tsc_codegen_nested_object_prop_with_proptype_not_truncated() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import type { PropType } from 'vue'

interface CascaderNode { label: string }

defineProps({
  nodes: {
    type: Array as PropType<CascaderNode[]>,
    required: true,
  },
  index: {
    type: Number,
    required: true,
  },
})
</script><template/>"#,
    );
    eprintln!("\n=== Nested object PropType (cascader-like) ===\n{}\n", r);

    // Runtime section must have valid object syntax (not truncated)
    let comp_section = r.split("declare const").next().unwrap();
    assert!(
        comp_section.contains("nodes: { type: Array, required: true }"),
        "nested object prop reconstructed cleanly: got {}",
        comp_section
    );
    assert!(
        comp_section.contains("index: { type: Number, required: true }"),
        "index prop intact"
    );
    assert!(
        !comp_section.contains("as PropType"),
        "no PropType cast in runtime"
    );

    // Type section
    assert!(r.contains("nodes: CascaderNode[]"), "required prop type");
    assert!(r.contains("index: number"), "required number type");
}

// ══════════════════════════════════════════════════════════════════════════════
// ── Step 2: Type Import Statements ──────────────────────────────────────────
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn tsc_codegen_type_import_emits_import_statement() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script><template/>"#,
    );

    // Positive: should emit a proper import type statement
    assert!(
        r.contains("import type { Props } from './types'"),
        "should emit import type statement: got {}",
        r
    );
    // Positive: $props should reference the type name directly
    assert!(
        r.contains("$props: import(\"vue\").PublicProps & Props"),
        "should use type name directly in $props: got {}",
        r
    );
    // Negative: should NOT use inline import() syntax anymore
    assert!(
        !r.contains("import('./types').Props"),
        "should not use inline import() syntax: got {}",
        r
    );
}

#[test]
fn tsc_codegen_type_import_specifier_level() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import { type MyProps, someValue } from './shared'
defineProps<MyProps>()
</script><template/>"#,
    );

    // Should emit type import for the type-only specifier
    assert!(
        r.contains("import type { MyProps } from './shared'"),
        "should emit import type for specifier-level type: got {}",
        r
    );
    assert!(
        r.contains("$props: import(\"vue\").PublicProps & MyProps"),
        "should reference type name directly: got {}",
        r
    );
    // Negative: should NOT import the value binding
    assert!(
        !r.contains("someValue"),
        "should not import the value binding: got {}",
        r
    );
}

#[test]
fn tsc_codegen_define_emits_imported_type_emits_import_statement() {
    let r = gen_tsc_with_external_type(
        r#"<script setup lang="ts">
import type { Emits } from './types'
defineEmits<Emits>()
</script><template/>"#,
        "Emits",
        "export interface Emits { (e: 'submit', payload: string): void; confirm: [id: number] }",
    );

    assert!(
        r.contains("import type { Emits } from './types'"),
        "defineEmits imported type should emit import type statement: {r}"
    );
    assert!(
        r.contains("((event: 'submit', payload: string) => void)"),
        "defineEmits imported type should inline call-signature overloads in $emit: {r}"
    );
    assert!(
        r.contains("((event: 'confirm', ...args: [id: number]) => void)"),
        "defineEmits imported type should inline shorthand overloads in $emit: {r}"
    );
    assert!(
        r.contains(r#""onSubmit"?: (payload: string) => void"#),
        "defineEmits imported type should inline submit handler props: {r}"
    );
    assert!(
        r.contains(r#""onConfirm"?: (...args: [id: number]) => void"#),
        "defineEmits imported type should inline confirm handler props: {r}"
    );
}

#[test]
fn tsc_codegen_define_emits_local_type_dependencies_are_emitted() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
interface Payload {
  value: string
}

interface Emits {
  (e: 'submit', payload: Payload): void
}

defineEmits<Emits>()
</script><template/>"#,
    );

    assert!(
        r.contains("interface Emits"),
        "defineEmits local type should emit the root declaration: {r}"
    );
    assert!(
        r.contains("interface Payload"),
        "defineEmits local type should emit transitive local declarations: {r}"
    );
}

#[test]
fn tsc_codegen_type_import_with_defaults() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import type { Props } from './types'
withDefaults(defineProps<Props>(), { title: 'hello' })
</script><template/>"#,
    );

    // Should emit import type statement even through withDefaults
    assert!(
        r.contains("import type { Props } from './types'"),
        "should emit import type with withDefaults: got {}",
        r
    );
    // $props should preserve the named type while optionalizing defaulted keys
    assert!(
        r.contains(
            "$props: import(\"vue\").PublicProps & Omit<Props, 'title'> & Partial<Pick<Props, 'title'>>"
        ),
        "should wrap named props when defaults are present: got {}",
        r
    );
}

#[test]
fn tsc_codegen_no_unused_type_imports() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import type { UnusedType } from './unused'
import type { Props } from './types'
defineProps<Props>()
</script><template/>"#,
    );

    // Should NOT emit unused type imports
    assert!(
        !r.contains("UnusedType"),
        "should not emit unused type import: got {}",
        r
    );
    assert!(
        !r.contains("'./unused'"),
        "should not reference unused source: got {}",
        r
    );
    // Should emit the used one
    assert!(
        r.contains("import type { Props } from './types'"),
        "should emit used type import: got {}",
        r
    );
}

// ── Step 3: JSDoc Comments on Props ─────────────────────────────────────────

#[test]
fn tsc_codegen_jsdoc_on_props() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineProps<{
  /** The title of the component */
  title: string
  /** The count value */
  count: number
}>()
</script><template/>"#,
    );

    // Positive: JSDoc comments preserved on props
    assert!(
        r.contains("/** The title of the component */"),
        "should preserve JSDoc on title: got {}",
        r
    );
    assert!(
        r.contains("/** The count value */"),
        "should preserve JSDoc on count: got {}",
        r
    );
    assert!(r.contains("title: string"), "title prop present");
    assert!(r.contains("count: number"), "count prop present");
}

#[test]
fn tsc_codegen_jsdoc_multiline() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineProps<{
  /**
   * The title of the component.
   * @default 'hello'
   */
  title: string
}>()
</script><template/>"#,
    );

    // Multi-line JSDoc should be preserved
    assert!(
        r.contains("* The title of the component."),
        "should preserve multi-line JSDoc: got {}",
        r
    );
    assert!(
        r.contains("@default 'hello'"),
        "should preserve @default tag: got {}",
        r
    );
}

#[test]
fn tsc_codegen_no_jsdoc_on_type_ref() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script><template/>"#,
    );

    // No JSDoc should be present when using external type reference
    // (the external type carries its own docs)
    assert!(
        !r.contains("/**"),
        "should not have JSDoc on type ref: got {}",
        r
    );
}

// ── Step 4: Slots Support ───────────────────────────────────────────────────

#[test]
fn tsc_codegen_define_slots_inline() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineSlots<{
  default(props: { item: string }): any
  header(): any
}>()
</script><template/>"#,
    );

    // Positive: $slots in output
    assert!(r.contains("$slots:"), "should emit $slots: got {}", r);
    assert!(
        r.contains("default(props: { item: string }): any"),
        "should have default slot type: got {}",
        r
    );
    assert!(
        r.contains("header(): any"),
        "should have header slot type: got {}",
        r
    );
    // Negative: no defineSlots in output
    assert!(
        !r.contains("defineSlots"),
        "defineSlots macro should be removed: got {}",
        r
    );
}

#[test]
fn tsc_codegen_define_slots_imported() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import type { MySlots } from './slots'
defineSlots<MySlots>()
</script><template/>"#,
    );

    // Should import the type and reference it
    assert!(
        r.contains("import type { MySlots } from './slots'"),
        "should emit import for slot type: got {}",
        r
    );
    assert!(
        r.contains("$slots: MySlots"),
        "should reference slot type by name: got {}",
        r
    );
}

#[test]
fn tsc_codegen_no_slots_when_not_defined() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineProps<{ title: string }>()
</script><template/>"#,
    );

    // No $slots: field inside the instance when defineSlots not used.
    // Note: "$slots" appears in the Omit<CPI, ...> exclusion list, but
    // the actual `$slots:` assignment must NOT appear in the instance body.
    assert!(
        !r.contains("$slots:"),
        "should not emit $slots: field without defineSlots: got {}",
        r
    );
}

#[test]
fn tsc_codegen_define_slots_local_interface() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
interface MySlots {
  default(props: { msg: string }): any
}
defineSlots<MySlots>()
</script><template/>"#,
    );

    // Should include local type declaration and reference it
    assert!(
        r.contains("interface MySlots"),
        "should include local interface: got {}",
        r
    );
    assert!(
        r.contains("$slots: MySlots"),
        "should reference local slot type: got {}",
        r
    );
}

// ── Step 5: Generic Component Support ───────────────────────────────────────

#[test]
fn tsc_codegen_generic_basic() {
    let r = gen_tsc(
        r#"<script setup lang="ts" generic="T">
defineProps<{ items: T[] }>()
</script><template/>"#,
    );

    // Positive: generic on new() with props param
    assert!(
        r.contains("new<T>(props?"),
        "should emit generic on new(props?): got {}",
        r
    );
    assert!(
        r.contains("items: T[]"),
        "should preserve generic type param in props: got {}",
        r
    );
    // Negative: should NOT have plain new() without generic
    assert!(
        !r.contains("  new()"),
        "should not have non-generic new(): got {}",
        r
    );
}

#[test]
fn tsc_codegen_generic_with_constraints() {
    let r = gen_tsc(
        r#"<script setup lang="ts" generic="T extends string">
defineProps<{ value: T }>()
</script><template/>"#,
    );

    assert!(
        r.contains("new<T extends string>(props?"),
        "should emit generic with constraint and props: got {}",
        r
    );
}

#[test]
fn tsc_codegen_generic_multiple() {
    let r = gen_tsc(
        r#"<script setup lang="ts" generic="K extends string, V">
defineProps<{ key: K; value: V }>()
</script><template/>"#,
    );

    assert!(
        r.contains("new<K extends string, V>(props?"),
        "should emit multiple generic params with props: got {}",
        r
    );
}

#[test]
fn tsc_codegen_no_generic_no_angle_brackets() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineProps<{ title: string }>()
</script><template/>"#,
    );

    // Without generic, should have plain new(props?: ...) constructor
    assert!(
        r.contains("new(props?: import(\"vue\").PublicProps &"),
        "should have new(props?) with PublicProps: got {}",
        r
    );
    assert!(
        !r.contains("new<"),
        "should not have angle brackets without generic: got {}",
        r
    );
    // Negative: no more Omit<CPI<...>> wrapper
    assert!(
        !r.contains("Omit<import(\"vue\").ComponentPublicInstance<"),
        "should not use Omit<CPI<...>> pattern: got {}",
        r
    );
}

#[test]
fn tsc_codegen_recursive_prop_types_no_excessive_depth() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
export interface Action { label: string; callback?: (a: Action) => void }
defineProps<{ actions: Action[] }>()
</script><template/>"#,
    );
    // Positive: constructor accepts props param
    assert!(
        r.contains("new(props?:"),
        "constructor accepts props param: got {}",
        r
    );
    // Negative: no ComponentPublicInstance in return type (causes excessive depth)
    assert!(
        !r.contains("ComponentPublicInstance"),
        "no CPI in output — avoids excessive depth: got {}",
        r
    );
    // Negative: no Omit<CPI<...>> wrapping that causes excessive depth
    assert!(
        !r.contains("Omit<import(\"vue\").ComponentPublicInstance<"),
        "no Omit<CPI<...>> pattern: got {}",
        r
    );
}

#[test]
fn tsc_codegen_transitive_local_type_dependencies_are_emitted() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
interface Role {
  name: string
}

interface User {
  role: Role
}

interface Props {
  user: User
}

defineProps<Props>()
</script><template/>"#,
    );

    assert!(
        r.contains("interface Props"),
        "Props should be emitted: {r}"
    );
    assert!(
        r.contains("interface User"),
        "User should be emitted transitively: {r}"
    );
    assert!(
        r.contains("interface Role"),
        "Role should be emitted transitively: {r}"
    );
}

// ── attrs attribute on <script setup> ────────────────────────────────────────

#[test]
fn tsc_codegen_attrs_explicit_type() {
    let r = gen_tsc(
        r#"<script setup lang="ts" attrs="{ class?: string; id?: string }">
defineProps<{ title: string }>()
</script><template/>"#,
    );

    // Positive: $attrs should contain the explicit type
    assert!(
        r.contains("$attrs: { class?: string; id?: string }"),
        "should emit explicit attrs type in $attrs: got {}",
        r
    );
    // Negative: should not be empty
    assert!(
        !r.contains("$attrs: {}"),
        "should not have empty $attrs with explicit attrs: got {}",
        r
    );
}

#[test]
fn tsc_codegen_attrs_default_empty() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineProps<{ title: string }>()
</script><template/>"#,
    );

    // Positive: $attrs should default to empty
    assert!(
        r.contains("$attrs: {}"),
        "should emit empty $attrs by default: got {}",
        r
    );
}

#[test]
fn tsc_codegen_attrs_alias_attributes() {
    let r = gen_tsc(
        r#"<script setup lang="ts" attributes="{ role?: string }">
defineProps<{ title: string }>()
</script><template/>"#,
    );

    // Positive: 'attributes' alias should work
    assert!(
        r.contains("$attrs: { role?: string }"),
        "'attributes' alias should produce typed $attrs: got {}",
        r
    );
}

#[test]
fn tsc_codegen_attrs_with_generic() {
    let r = gen_tsc(
        r#"<script setup lang="ts" generic="T" attrs="{ value: T }">
defineProps<{ items: T[] }>()
</script><template/>"#,
    );

    // Positive: $attrs should contain the generic type
    assert!(
        r.contains("$attrs: { value: T }"),
        "should emit generic attrs type in $attrs: got {}",
        r
    );
}

#[test]
fn tsc_codegen_attrs_imported_named_type_emits_import_statement() {
    let r = gen_tsc(
        r#"<script setup lang="ts" attrs="Attrs">
import type { Attrs } from './types'
</script><template/>"#,
    );

    assert!(
        r.contains("import type { Attrs } from './types'"),
        "named attrs type should emit import type statement: {r}"
    );
    assert!(
        r.contains("$attrs: Attrs"),
        "named attrs type should be preserved: {r}"
    );
}

#[test]
fn tsc_codegen_use_attrs_type_arg_fallback() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import { useAttrs } from 'vue'
const attrs = useAttrs<{ class?: string; id?: string }>()
</script><template/>"#,
    );

    // Positive: useAttrs<T>() type parameter used as $attrs type
    assert!(
        r.contains("$attrs: { class?: string; id?: string }"),
        "should use useAttrs type param as $attrs type, got:\n{}",
        r
    );
    // Negative: should not have empty $attrs
    assert!(
        !r.contains("$attrs: {},"),
        "should not have empty $attrs when useAttrs<T> provides type, got:\n{}",
        r
    );
}

#[test]
fn tsc_codegen_use_attrs_local_type_dependencies_are_emitted() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import { useAttrs } from 'vue'

interface Role {
  label: string
}

interface Attrs {
  role: Role
}

const attrs = useAttrs<Attrs>()
</script><template/>"#,
    );

    assert!(
        r.contains("$attrs: Attrs"),
        "named attrs type should be preserved: {r}"
    );
    assert!(
        r.contains("interface Attrs"),
        "Attrs interface should be emitted: {r}"
    );
    assert!(
        r.contains("interface Role"),
        "Role interface should be emitted transitively: {r}"
    );
}

#[test]
fn tsc_codegen_dedupes_shared_named_type_references_across_surfaces() {
    let r = gen_tsc(
        r#"<script setup lang="ts" attrs="Shared">
import type { Shared } from './types'
const model = defineModel<Shared>()
</script><template/>"#,
    );

    let count = r.matches("import type { Shared } from './types'").count();
    assert_eq!(
        count, 1,
        "Shared import should be emitted once across surfaces: {r}"
    );
}

#[test]
fn tsc_codegen_attrs_attribute_priority_over_use_attrs() {
    let r = gen_tsc(
        r#"<script setup lang="ts" attrs="{ role?: string }">
import { useAttrs } from 'vue'
const attrs = useAttrs<{ class?: string }>()
</script><template/>"#,
    );

    // Positive: attrs attribute takes priority
    assert!(
        r.contains("$attrs: { role?: string }"),
        "attrs attribute should take priority over useAttrs<T>, got:\n{}",
        r
    );
    // Negative: useAttrs type should not appear in $attrs
    assert!(
        !r.contains("class?: string"),
        "useAttrs type param should not override attrs attribute, got:\n{}",
        r
    );
}

#[test]
fn tsc_codegen_use_attrs_without_type_no_effect() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import { useAttrs } from 'vue'
const attrs = useAttrs()
</script><template/>"#,
    );

    // Positive: plain useAttrs() → default empty $attrs
    assert!(
        r.contains("$attrs: {},"),
        "useAttrs() without type param should produce empty $attrs, got:\n{}",
        r
    );
}

// ── Root element attrs in external $attrs ────────────────────────────────────

#[test]
fn tsc_root_element_attrs_native_html_root() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
const x = 1
</script><template><div>hello</div></template>"#,
    );
    // Positive: native HTML root should give HTMLAttributes
    assert!(
        r.contains("$attrs: import(\"vue\").HTMLAttributes"),
        "native HTML root should have HTMLAttributes in $attrs, got:\n{}",
        r
    );
    // Negative: should NOT be empty
    assert!(
        !r.contains("$attrs: {},"),
        "$attrs should NOT be empty when native HTML root exists, got:\n{}",
        r
    );
}

#[test]
fn tsc_root_element_attrs_inherit_attrs_false() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineOptions({ inheritAttrs: false })
</script><template><div>hello</div></template>"#,
    );
    // Positive: inheritAttrs: false should give empty $attrs
    assert!(
        r.contains("$attrs: {},"),
        "inheritAttrs: false should have empty $attrs, got:\n{}",
        r
    );
    // Negative: should NOT have HTMLAttributes
    assert!(
        !r.contains("HTMLAttributes"),
        "inheritAttrs: false should NOT have HTMLAttributes, got:\n{}",
        r
    );
}

#[test]
fn tsc_root_element_attrs_explicit_takes_precedence() {
    let r = gen_tsc(
        r#"<script setup lang="ts" attrs="{ class?: string }">
const x = 1
</script><template><div>hello</div></template>"#,
    );
    // Positive: explicit attrs should take precedence
    assert!(
        r.contains("$attrs: { class?: string }"),
        "explicit attrs should take precedence over root element, got:\n{}",
        r
    );
    // Negative: should NOT have HTMLAttributes
    assert!(
        !r.contains("HTMLAttributes"),
        "explicit attrs should NOT include HTMLAttributes, got:\n{}",
        r
    );
}

#[test]
fn tsc_root_element_attrs_component_root() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
</script><template><MyComp /></template>"#,
    );
    // Positive: component root should give empty $attrs (can't resolve type)
    assert!(
        r.contains("$attrs: {},"),
        "component root should have empty $attrs, got:\n{}",
        r
    );
}

#[test]
fn tsc_root_element_attrs_fragment() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
const x = 1
</script><template><div>A</div><span>B</span></template>"#,
    );
    // Positive: fragment should give empty $attrs
    assert!(
        r.contains("$attrs: {},"),
        "fragment root should have empty $attrs, got:\n{}",
        r
    );
}

#[test]
fn tsc_root_element_attrs_no_template() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
const x = 1
</script>"#,
    );
    // Positive: no template should give empty $attrs
    assert!(
        r.contains("$attrs: {},"),
        "no template should have empty $attrs, got:\n{}",
        r
    );
}

// ── Barrel export type preservation: __OmitNew + Omit<CPI> ──────────────────
//
// Barrel re-exports (`export { default as X } from './X.vue'`) degrade
// `typeof __comp`'s construct signature, picking `DefineComponent<{}>`'s
// empty `$props` over our explicit typed one. The fix:
// 1. `__OmitNew<typeof __comp>` strips the construct sig via mapped type
// 2. A single `new()` returns `Omit<CPI, ...> & { $props: T, $emit: E, ... }`
//    so barrel re-exports have exactly one construct signature.

#[test]
fn tsc_codegen_uses_omit_new_for_barrel_safety() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineProps<{
  zIndex?: number
  duration?: number | string
  show?: boolean
  lockScroll?: boolean
}>()
</script><template><div /></template>"#,
    );

    eprintln!("\n=== barrel type fix output ===\n{}\n", r);

    // Positive: __OmitNew utility type is emitted
    assert!(
        r.contains("type __OmitNew<T> = { [K in keyof T]: T[K] }"),
        "__OmitNew utility type should be present: got:\n{}",
        r
    );
    // Positive: declare uses __OmitNew<typeof __comp>, not raw typeof __comp
    assert!(
        r.contains("__OmitNew<typeof __comp>"),
        "should use __OmitNew<typeof __comp>: got:\n{}",
        r
    );
    // Negative: raw `typeof __comp &` intersection must NOT appear
    assert!(
        !r.contains(": typeof __comp &"),
        "should NOT use raw typeof __comp in intersection: got:\n{}",
        r
    );
    // Positive: CPI is used as plain intersection in constructor return type
    assert!(
        r.contains("new(props?: import(\"vue\").PublicProps &"),
        "constructor should accept props with PublicProps: got:\n{}",
        r
    );
    // Negative: no more Omit<CPI<...>> wrapping
    assert!(
        !r.contains("Omit<import(\"vue\").ComponentPublicInstance<"),
        "should NOT use Omit<CPI<...>> pattern: got:\n{}",
        r
    );
    // Positive: $props includes PublicProps for class/style/key
    assert!(
        r.contains("$props: import(\"vue\").PublicProps &"),
        "$props should include PublicProps intersection: got:\n{}",
        r
    );
    // Positive: explicit $props still has typed fields
    assert!(
        r.contains("zIndex?: number"),
        "$props should have typed zIndex: got:\n{}",
        r
    );
    assert!(
        r.contains("show?: boolean"),
        "$props should have typed show: got:\n{}",
        r
    );
}

#[test]
fn tsc_codegen_generic_uses_omit_new() {
    let r = gen_tsc(
        r#"<script setup lang="ts" generic="T">
defineProps<{ items: T[] }>()
</script><template/>"#,
    );

    // Positive: generic on new() with props param
    assert!(
        r.contains("new<T>(props?: import(\"vue\").PublicProps &"),
        "generic new() should accept props with PublicProps: got:\n{}",
        r
    );
    // Negative: no more Omit<CPI<...>> wrapping
    assert!(
        !r.contains("Omit<import(\"vue\").ComponentPublicInstance<"),
        "should NOT use Omit<CPI<...>> pattern: got:\n{}",
        r
    );
    assert!(
        r.contains("__OmitNew<typeof __comp>"),
        "should use __OmitNew: got:\n{}",
        r
    );
}

// ── Conditional root narrowing ──────────────────────────────────────────────

#[test]
fn tsc_narrowing_basic() {
    let r = gen_tsc_narrowing(
        r#"<script setup lang="ts">
defineProps<{foo?: boolean}>()
</script>
<template><div v-if="foo">A</div><span v-else>B</span></template>"#,
    );
    // Positive: narrowing generic on new()
    assert!(
        r.contains("T_foo extends boolean = boolean"),
        "should have T_foo generic: {r}"
    );
    // Positive: $props uses generic type
    assert!(
        r.contains("foo?: T_foo"),
        "should substitute generic in $props: {r}"
    );
    // Positive: $root with conditional type
    assert!(
        r.contains("$root: T_foo extends true ? HTMLDivElement : HTMLSpanElement"),
        "$root should have conditional type: {r}"
    );
}

#[test]
fn tsc_narrowing_multi() {
    let r = gen_tsc_narrowing(
        r#"<script setup lang="ts">
defineProps<{foo?: boolean, s?: 'foo' | 'bar'}>()
</script>
<template><div v-if="foo">A</div><span v-else-if="s === 'foo'">B</span><canvas v-else-if="s === 'bar'">C</canvas><input v-else /></template>"#,
    );
    assert!(
        r.contains("T_foo extends boolean = boolean"),
        "should have T_foo: {r}"
    );
    assert!(
        r.contains("T_s extends 'foo' | 'bar' = 'foo' | 'bar'"),
        "should have T_s: {r}"
    );
    assert!(r.contains("$root:"), "should have $root: {r}");
}

#[test]
fn tsc_narrowing_with_sfc_generics() {
    let r = gen_tsc_narrowing(
        r#"<script setup lang="ts" generic="T extends string">
defineProps<{show?: boolean}>()
</script>
<template><div v-if="show">A</div><span v-else>B</span></template>"#,
    );
    // Both existing generic and narrowing generic
    assert!(
        r.contains("T extends string, T_show extends boolean = boolean"),
        "should append narrowing to existing generics: {r}"
    );
}

#[test]
fn tsc_narrowing_disabled() {
    // Use default (narrowing disabled)
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineProps<{foo?: boolean}>()
</script>
<template><div v-if="foo">A</div><span v-else>B</span></template>"#,
    );
    assert!(
        !r.contains("T_foo"),
        "should NOT have narrowing when disabled: {r}"
    );
    assert!(
        !r.contains("$root"),
        "should NOT have $root when disabled: {r}"
    );
}

#[test]
fn tsc_narrowing_component_roots() {
    let r = gen_tsc_narrowing(
        r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
import Other from './Other.vue'
defineProps<{v?: 'a' | 'b'}>()
</script>
<template><MyComp v-if="v === 'a'" /><Other v-else /></template>"#,
    );
    assert!(r.contains("T_v extends"), "should have T_v generic: {r}");
    assert!(
        r.contains("InstanceType<typeof MyComp>"),
        "$root should use InstanceType for components: {r}"
    );
    assert!(
        r.contains("InstanceType<typeof Other>"),
        "$root should use InstanceType for Other: {r}"
    );
}

// ── Emits-to-props: emit events should appear as onEventName in $props ───────

#[test]
fn tsc_codegen_emits_array_to_props() {
    let r = gen_tsc(
        r#"<script setup>
defineEmits(['change', 'clickOverlay'])
</script><template/>"#,
    );

    // Positive: emit events become onEventName props
    assert!(
        r.contains(r#""onChange"?:"#),
        "should have onChange in $props: {r}"
    );
    assert!(
        r.contains(r#""onClickOverlay"?:"#),
        "should have onClickOverlay in $props: {r}"
    );
}

#[test]
fn tsc_codegen_typed_emits_to_props() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineEmits<{ (e: 'click', event: MouseEvent): void }>()
</script><template/>"#,
    );

    assert!(
        r.contains(r#""onClick"?: (event: MouseEvent) => void"#),
        "type-based emits should inline handler props with preserved payload types: {r}"
    );
    assert!(
        r.contains("((event: 'click', event: MouseEvent) => void)"),
        "type-based emits should inline $emit overloads: {r}"
    );
}

#[test]
fn tsc_codegen_emits_and_models_props() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineEmits<{ (e: 'submit', data: string): void }>()
const model = defineModel<string>()
</script><template/>"#,
    );

    assert!(
        r.contains(r#""onSubmit"?: (data: string) => void"#),
        "type-based emits should contribute inline handler props: {r}"
    );
    assert!(
        r.contains("modelValue?:"),
        "should have modelValue prop: {r}"
    );
    assert!(
        r.contains(r#""onUpdate:modelValue"?:"#),
        "should have onUpdate:modelValue prop: {r}"
    );
}

// ── Kebab-case emit → dual $props keys ──────────────────────────────────────

// Type-based defineEmits: handler types should be inferred from the original type
// via __EmitToProps<OriginalType> rather than manual (...args: unknown[]) => void
#[test]
fn tsc_codegen_kebab_emit_type_based_both_keys_with_correct_handler() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineEmits<{ (e: 'my-event', value: string): void }>()
</script><template/>"#,
    );

    assert!(
        r.contains(r#""onMy-event"?: (value: string) => void"#),
        "kebab emits should generate the kebab handler key inline: {r}"
    );
    assert!(
        r.contains(r#""onMyEvent"?: (value: string) => void"#),
        "kebab emits should also generate the camel handler key inline: {r}"
    );
    assert!(
        !r.contains("__EmitToProps<") && !r.contains("type __Cam<"),
        "inline emits should not rely on helper aliases anymore: {r}"
    );
}

// Type-based defineEmits with multi-segment kebab
#[test]
fn tsc_codegen_multi_segment_kebab_emit_type_based() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineEmits<{ (e: 'my-custom-event'): void }>()
</script><template/>"#,
    );

    assert!(
        r.contains(r#""onMy-custom-event"?: () => void"#),
        "multi-segment kebab emits should keep the kebab alias: {r}"
    );
    assert!(
        r.contains(r#""onMyCustomEvent"?: () => void"#),
        "multi-segment kebab emits should also generate the camel alias: {r}"
    );
}

// Object-syntax defineEmits: handler type inferred from validator params
#[test]
fn tsc_codegen_kebab_emit_object_syntax_both_keys() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineEmits({
  'my-event': (value: string) => true,
  'click': (id: number) => true,
})
</script><template/>"#,
    );

    assert!(
        r.contains(r#""onMy-event"?: (value: string) => void"#),
        "object emits should inline the kebab handler key: {r}"
    );
    assert!(
        r.contains(r#""onMyEvent"?: (value: string) => void"#),
        "object emits should inline the camel handler key: {r}"
    );
    assert!(
        r.contains("((event: 'my-event', value: string) => void)"),
        "object emits should inline $emit overloads from validator params: {r}"
    );
}

// Array-syntax defineEmits
#[test]
fn tsc_codegen_kebab_emit_array_syntax_both_keys() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineEmits(['my-event', 'click'])
</script><template/>"#,
    );

    // kebab event → both keys
    assert!(
        r.contains(r#""onMy-event"?:"#),
        "should have capitalize-only onMy-event prop: {r}"
    );
    assert!(
        r.contains(r#""onMyEvent"?:"#),
        "should have camelized onMyEvent prop: {r}"
    );
    // non-kebab event → single key per props block (appears in both new() param and $props)
    assert!(
        r.contains(r#""onClick"?:"#),
        "should have onClick prop: {r}"
    );
    let count = r.matches(r#""onClick"?:"#).count();
    assert_eq!(
        count, 2,
        "non-kebab emit should produce exactly one key per props block (2 total: new() + $props): {r}"
    );
}

// camelCase emit (type-based) → camel + kebab handler aliases
#[test]
fn tsc_codegen_camel_emit_no_duplicate_prop() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineEmits<{ (e: 'myEvent'): void }>()
</script><template/>"#,
    );

    assert!(
        r.contains(r#""onMyEvent"?: () => void"#),
        "camel emits should keep the camel handler key: {r}"
    );
    assert!(
        r.contains(r#""onMy-event"?: () => void"#),
        "camel emits should also generate the kebab handler key: {r}"
    );
}

// Simple emit (type-based) → single deduped handler key per props block
#[test]
fn tsc_codegen_simple_emit_no_duplicate_prop() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineEmits<{ (e: 'click'): void }>()
</script><template/>"#,
    );

    let count = r.matches(r#""onClick"?: () => void"#).count();
    assert_eq!(
        count, 2,
        "simple emits should only produce one deduped handler key in new() and $props: {r}"
    );
}

// update: prefix (type-based) → colon form only
#[test]
fn tsc_codegen_update_prefix_emit_no_camelize() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineEmits<{ (e: 'update:modelValue'): void }>()
</script><template/>"#,
    );

    assert!(
        r.contains(r#""onUpdate:modelValue"?: () => void"#),
        "colon emits should keep the colon handler key: {r}"
    );
    assert!(
        !r.contains("onUpdateModelValue"),
        "colon emits should not generate camelized aliases: {r}"
    );
}

// ── Shorthand type-based defineEmits: $emit + $props typing ──────────────────

#[test]
fn tsc_codegen_shorthand_emits_emit_type_uses_emit_fn() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineEmits<{
  change: [value: string];
  update: [id: number, data: { name: string }];
}>()
</script><template/>"#,
    );

    assert!(
        r.contains("((event: 'change', ...args: [value: string]) => void)"),
        "shorthand emits should inline tuple overloads in $emit: {r}"
    );
    assert!(
        r.contains("((event: 'update', ...args: [id: number, data: { name: string }]) => void)"),
        "shorthand emits should preserve tuple payload text in $emit: {r}"
    );
    assert!(
        !r.contains("__EmitFn<"),
        "helper emit aliases should be gone: {r}"
    );
}

#[test]
fn tsc_codegen_shorthand_emits_props_uses_emit_to_props() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineEmits<{
  change: [value: string];
}>()
</script><template/>"#,
    );

    assert!(
        r.contains(r#""onChange"?: (...args: [value: string]) => void"#),
        "shorthand emits should inline tuple handler props: {r}"
    );
}

// Function-form type-based: $emit should also inline overloads
#[test]
fn tsc_codegen_function_form_emits_emit_type_uses_emit_fn() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineEmits<{ (e: 'click', event: MouseEvent): void }>()
</script><template/>"#,
    );

    assert!(
        r.contains("((event: 'click', event: MouseEvent) => void)"),
        "function-form emits should inline $emit overloads: {r}"
    );
    assert!(
        !r.contains("__EmitFn<"),
        "helper emit aliases should be gone: {r}"
    );
}

// ── Object-arg defineEmits: $emit + $props typing ────────────────────────────

#[test]
fn tsc_codegen_object_arg_emits_uses_type_helpers() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineEmits({
  change: (value: string) => true,
  submit: null,
})
</script><template/>"#,
    );

    assert!(
        r.contains(r#""onChange"?: (value: string) => void"#),
        "object-arg emits should inline handler props: {r}"
    );
    assert!(
        r.contains("((event: 'submit', ...args: unknown[]) => void)"),
        "null validators should fall back to unknown[] in $emit: {r}"
    );
    assert!(
        !r.contains("__EmitToProps<") && !r.contains("__EmitFn<"),
        "object-arg emits should no longer use helper aliases: {r}"
    );
}

#[test]
fn tsc_sourcemap_emit_handler_prop_maps_to_event_name() {
    let sfc = r#"<script setup lang="ts">
defineEmits<{ (e: 'my-event', value: string): void }>()
</script><template/>"#;
    let out = gen_tsc_output(sfc);
    let sourcemap = SourceMap::from_json_string(&out.source_map).expect("valid source map");
    let lookup = sourcemap.generate_lookup_table();
    let generated_offset = out
        .code
        .find(r#""onMyEvent""#)
        .expect("generated handler prop");
    let (generated_line, generated_col) =
        offset_to_zero_based_line_col(&out.code, generated_offset);
    let token = sourcemap
        .lookup_source_view_token(&lookup, generated_line, generated_col)
        .expect("mapped handler prop token");
    let expected_offset = sfc.find("'my-event'").expect("source event literal");
    let (expected_line, expected_col) = offset_to_zero_based_line_col(sfc, expected_offset);

    assert_eq!(token.get_source(), Some("/test/TestComp.vue"));
    assert_eq!(token.get_src_line(), expected_line);
    assert_eq!(token.get_src_col(), expected_col);
}

#[test]
fn tsc_sourcemap_prop_key_maps_to_prop_name() {
    let sfc = r#"<script setup lang="ts">
defineProps<{ title: string; count?: number }>()
</script><template/>"#;
    let out = gen_tsc_output(sfc);
    let sourcemap = SourceMap::from_json_string(&out.source_map).expect("valid source map");
    let lookup = sourcemap.generate_lookup_table();
    let generated_offset = out.code.find("title: string").expect("generated prop");
    let (generated_line, generated_col) =
        offset_to_zero_based_line_col(&out.code, generated_offset);
    let token = sourcemap
        .lookup_source_view_token(&lookup, generated_line, generated_col)
        .expect("mapped prop token");
    let expected_offset = sfc.find("title: string").expect("source prop");
    let (expected_line, expected_col) = offset_to_zero_based_line_col(sfc, expected_offset);

    assert_eq!(token.get_source(), Some("/test/TestComp.vue"));
    assert_eq!(token.get_src_line(), expected_line);
    assert_eq!(token.get_src_col(), expected_col);
}

#[test]
fn tsc_sourcemap_model_members_map_to_model_name() {
    let sfc = r#"<script setup lang="ts">
const title = defineModel<string>('title')
</script><template/>"#;
    let out = gen_tsc_output(sfc);
    let sourcemap = SourceMap::from_json_string(&out.source_map).expect("valid source map");
    let lookup = sourcemap.generate_lookup_table();
    let generated_offset = out
        .code
        .find(r#""onUpdate:title""#)
        .expect("generated model handler");
    let (generated_line, generated_col) =
        offset_to_zero_based_line_col(&out.code, generated_offset);
    let token = sourcemap
        .lookup_source_view_token(&lookup, generated_line, generated_col)
        .expect("mapped model token");
    let expected_offset = sfc.find("'title'").expect("source model name");
    let (expected_line, expected_col) = offset_to_zero_based_line_col(sfc, expected_offset);

    assert_eq!(token.get_source(), Some("/test/TestComp.vue"));
    assert_eq!(token.get_src_line(), expected_line);
    assert_eq!(token.get_src_col(), expected_col);
}

// ── Extract + Generate cache equivalence tests ───────────────────────────

use super::script::{extract_tsc_state, generate_tsc_from_state, TscExtractOptions};

#[test]
fn extract_then_generate_matches_direct_for_inline_types() {
    let sfc = r#"<script setup lang="ts">
defineProps<{ x: number; y?: string }>()
defineEmits<{ (e: 'change', val: number): void }>()
</script>
<template><div /></template>"#;

    let extracted = extract_tsc_state(
        sfc,
        "TestComp",
        &TscExtractOptions {
            filename: Some("/test/TestComp.vue".to_string()),
        },
    )
    .expect("should extract from SFC with script setup");

    let from_cache = generate_tsc_from_state(&extracted, sfc, "TestComp", TscMode::Public, None);
    let direct = gen_tsc(sfc);

    assert_eq!(
        from_cache.code, direct,
        "cached path must produce identical code to direct path"
    );
}

#[test]
fn extract_then_generate_matches_direct_for_runtime_macros() {
    let sfc = r#"<script setup lang="ts">
defineProps({ x: String, count: { type: Number, default: 0 } })
defineEmits(['click', 'update'])
</script>
<template><div /></template>"#;

    let extracted = extract_tsc_state(
        sfc,
        "TestComp",
        &TscExtractOptions {
            filename: Some("/test/TestComp.vue".to_string()),
        },
    )
    .expect("should extract from SFC with runtime macros");

    let from_cache = generate_tsc_from_state(&extracted, sfc, "TestComp", TscMode::Public, None);
    let direct = gen_tsc(sfc);

    assert_eq!(
        from_cache.code, direct,
        "cached path must match direct for runtime macros"
    );
}

#[test]
fn extract_cache_with_external_emits_matches_direct() {
    let sfc = r#"<script setup lang="ts">
import type { ImportedEmits } from './types'
defineEmits<ImportedEmits>()
</script>
<template><div /></template>"#;

    let dep_source = r#"
export interface ImportedEmits {
    (e: 'save', data: string): void
    (e: 'cancel'): void
}
"#;

    // Extract WITHOUT external types (as cache would)
    let extracted = extract_tsc_state(
        sfc,
        "TestComp",
        &TscExtractOptions {
            filename: Some("/test/TestComp.vue".to_string()),
        },
    )
    .expect("should extract");

    // Verify unresolved emits ref is recorded
    assert!(
        extracted.unresolved_emits_ref.is_some(),
        "should record unresolved emits type ref"
    );

    // Resolve external types
    let alloc = Allocator::default();
    let resolved = crate::utils::oxc::vue::resolve_type::resolve_external_type(
        "ImportedEmits",
        dep_source,
        &alloc,
    )
    .expect("resolve external type");
    let mut external_types = FxHashMap::default();
    external_types.insert("ImportedEmits".to_string(), resolved);

    // Generate from cache WITH external types
    let from_cache = generate_tsc_from_state(
        &extracted,
        sfc,
        "TestComp",
        TscMode::Public,
        Some(&external_types),
    );

    // Generate directly with external types
    let direct = gen_tsc_with_external_type(sfc, "ImportedEmits", dep_source);

    assert_eq!(
        from_cache.code, direct,
        "cached + external emits must match direct path"
    );
    assert!(
        !from_cache.code.contains("emits: []"),
        "should not have empty emits array"
    );
}

#[test]
fn extract_cache_with_external_props_testing_mode() {
    let sfc = r#"<script setup lang="ts">
import type { ImportedProps } from './types'
defineProps<ImportedProps>()
</script>
<template><div /></template>"#;

    let dep_source = r#"
export interface ImportedProps {
    title: string
    count?: number
}
"#;

    // Extract WITHOUT external types
    let extracted = extract_tsc_state(
        sfc,
        "TestComp",
        &TscExtractOptions {
            filename: Some("/test/TestComp.vue".to_string()),
        },
    )
    .expect("should extract");

    assert!(
        extracted.unresolved_props_ref.is_some(),
        "should record unresolved props type ref"
    );

    // Resolve external types
    let alloc = Allocator::default();
    let resolved = crate::utils::oxc::vue::resolve_type::resolve_external_type(
        "ImportedProps",
        dep_source,
        &alloc,
    )
    .expect("resolve external type");
    let mut external_types = FxHashMap::default();
    external_types.insert("ImportedProps".to_string(), resolved);

    // Generate from cache in Testing mode
    let from_cache = generate_tsc_from_state(
        &extracted,
        sfc,
        "TestComp",
        TscMode::Testing,
        Some(&external_types),
    );

    // Generate directly in Testing mode
    let direct = gen_tsc_testing_with_external_type(sfc, "ImportedProps", dep_source);

    assert_eq!(
        from_cache.code, direct,
        "cached Testing mode with external props must match direct"
    );
}

#[test]
fn extract_returns_none_without_script_setup() {
    let sfc = r#"<template><div>hello</div></template>"#;

    let result = extract_tsc_state(sfc, "TestComp", &TscExtractOptions::default());
    assert!(
        result.is_none(),
        "should return None for SFC without script setup"
    );
}

#[test]
fn extract_records_unresolved_refs() {
    let sfc = r#"<script setup lang="ts">
import type { Ext } from './external'
defineProps<Ext>()
</script>
<template><div /></template>"#;

    let extracted = extract_tsc_state(
        sfc,
        "TestComp",
        &TscExtractOptions {
            filename: Some("/test/TestComp.vue".to_string()),
        },
    )
    .expect("should extract");

    assert_eq!(
        extracted.unresolved_props_ref.as_deref(),
        Some("Ext"),
        "should record the unresolved props type name"
    );
    assert!(
        extracted.unresolved_emits_ref.is_none(),
        "should not have unresolved emits ref when no defineEmits"
    );
}

// ── TSC output with dotted component names ──────────────────────────

fn assert_valid_tsc_output(source: &str, name: &str) {
    let tsc_out = super::generate_tsc_output(source, name);
    let code = &tsc_out.code;
    eprintln!("=== TSC {} ===\n{}\n=== END ===", name, code);

    let alloc = oxc_allocator::Allocator::new();
    let parsed = oxc_parser::Parser::new(&alloc, code, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.errors {
        eprintln!("[TSC {name}] OXC ERROR: {err}");
    }
    assert!(
        parsed.errors.is_empty(),
        "[TSC {name}] should have no parse errors. Got {} errors. Output:\n{code}",
        parsed.errors.len()
    );
}

#[test]
fn tsc_dotted_component_name_sanitized() {
    // Dotted names like "Drawer.draggable" produce invalid identifiers in
    // `declare const Drawer.draggable:` — dots must be replaced with underscores.
    let source = r#"<script setup lang="ts">
defineProps<{ open: boolean }>()
</script>
<template><div>hello</div></template>"#;
    assert_valid_tsc_output(source, "Drawer.draggable");
}

#[test]
fn tsc_multi_dotted_component_name_sanitized() {
    // Multiple dots: "SwiperCardStyle.story.component"
    let source = r#"<script setup lang="ts">
defineProps<{ count: number }>()
</script>
<template><div>{{ count }}</div></template>"#;
    assert_valid_tsc_output(source, "SwiperCardStyle.story.component");
}

#[test]
fn tsc_dotted_name_produces_valid_identifiers() {
    let source = r#"<script setup lang="ts">
defineProps<{ value: string }>()
</script>
<template><div>{{ value }}</div></template>"#;
    let tsc_out = super::generate_tsc_output(source, "My.Component.Name");
    let code = &tsc_out.code;

    // The output must NOT contain bare `My.Component.Name` as an identifier
    assert!(
        !code.contains("const My.Component.Name"),
        "dotted name must be sanitized in const declaration: {code}"
    );
    assert!(
        !code.contains("default My.Component.Name"),
        "dotted name must be sanitized in export default: {code}"
    );
    // It SHOULD contain the sanitized version
    assert!(
        code.contains("My_Component_Name"),
        "dotted name should be sanitized to underscores: {code}"
    );
}

// ── defineExpose ──────────────────────────────────────────────────────────────

#[test]
fn tsc_codegen_define_expose_object_arg() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const foo = ref(1)
const bar = ref('hello')
function baz() {}
defineExpose({ foo, bar, baz })
</script><template/>"#,
    );

    // Positive: shorthand props use typeof inference via ShallowUnwrapRef
    assert!(
        r.contains("foo: typeof foo"),
        "should have foo: typeof foo in return type: {r}"
    );
    assert!(
        r.contains("bar: typeof bar"),
        "should have bar: typeof bar in return type: {r}"
    );
    assert!(
        r.contains("baz: typeof baz"),
        "should have baz: typeof baz in return type: {r}"
    );
    assert!(
        r.contains("ShallowUnwrapRef"),
        "should use ShallowUnwrapRef wrapper: {r}"
    );
}

#[test]
fn tsc_codegen_define_expose_empty() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineExpose()
</script><template/>"#,
    );

    // Should produce valid output with no extra properties
    assert!(r.contains("new("), "should have constructor in output: {r}");
    assert!(
        !r.contains("defineExpose("),
        "defineExpose call should be removed from output: {r}"
    );
}

#[test]
fn tsc_codegen_define_expose_type_param() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
const foo = ref(1)
const bar = ref('hello')
defineExpose<{ foo: number, bar: string }>({ foo, bar })
</script><template/>"#,
    );

    // Type param wins: intersection with the type text
    assert!(
        r.contains("{ foo: number, bar: string }"),
        "should have type text as intersection on return type: {r}"
    );
    // Should NOT have individual `foo: any` — type param covers it
    assert!(
        !r.contains("foo: any"),
        "should not have individual foo: any when type param present: {r}"
    );
    assert!(
        !r.contains("defineExpose("),
        "defineExpose call should be removed from output: {r}"
    );
}

#[test]
fn tsc_codegen_define_expose_non_object() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
const obj = { foo: 1 }
defineExpose(obj)
</script><template/>"#,
    );

    // Can't extract names from a variable reference — no exposed properties
    assert!(
        !r.contains("foo: any"),
        "should not have foo: any for non-object arg: {r}"
    );
    assert!(r.contains("new("), "should have constructor in output: {r}");
}

#[test]
fn tsc_codegen_define_expose_method() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
function greet(name: string) { return `Hello, ${name}!` }
defineExpose({ greet })
</script><template/>"#,
    );

    // Shorthand property with function identifier — uses typeof
    assert!(
        r.contains("greet: typeof greet"),
        "should have greet: typeof greet in return type: {r}"
    );
}

#[test]
fn tsc_codegen_define_expose_computed_key() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineExpose({ foo, bar: computed(() => 1), baz: 'literal' })
</script><template/>"#,
    );

    // foo is shorthand → typeof, others are complex → any
    assert!(
        r.contains("foo: typeof foo"),
        "should have foo: typeof foo: {r}"
    );
    assert!(r.contains("bar: any"), "should have bar: any: {r}");
    assert!(r.contains("baz: any"), "should have baz: any: {r}");
}

// ── defineExpose with typeof inference ────────────────────────────────────────

#[test]
fn tsc_define_expose_shorthand_uses_typeof() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import { ref, computed } from 'vue'
const foo = ref(1)
const bar = computed(() => 'hello')
defineExpose({ foo, bar })
</script><template/>"#,
    );

    // Positive: typeof inference via ShallowUnwrapRef
    assert!(
        r.contains("typeof foo"),
        "should use typeof for shorthand foo: {r}"
    );
    assert!(
        r.contains("typeof bar"),
        "should use typeof for shorthand bar: {r}"
    );
    assert!(
        r.contains("ShallowUnwrapRef"),
        "should use ShallowUnwrapRef wrapper: {r}"
    );
    // Negative: must NOT fall back to `any`
    assert!(
        !r.contains("foo: any"),
        "should NOT use any for shorthand foo: {r}"
    );
    assert!(
        !r.contains("bar: any"),
        "should NOT use any for shorthand bar: {r}"
    );
}

#[test]
fn tsc_define_expose_non_shorthand_ident_uses_typeof() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const val = ref(42)
defineExpose({ myVal: val })
</script><template/>"#,
    );

    assert!(
        r.contains("myVal: typeof val"),
        "should use typeof for identifier value: {r}"
    );
    assert!(
        !r.contains("myVal: any"),
        "should NOT use any for identifier value: {r}"
    );
}

#[test]
fn tsc_define_expose_method_shorthand_falls_back() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineExpose({ focus() {} })
</script><template/>"#,
    );

    assert!(
        r.contains("focus: any"),
        "method shorthand should fall back to any: {r}"
    );
}

#[test]
fn tsc_define_expose_complex_value_falls_back() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import { computed } from 'vue'
defineExpose({ bar: computed(() => 1) })
</script><template/>"#,
    );

    assert!(
        r.contains("bar: any"),
        "complex expression value should fall back to any: {r}"
    );
}

#[test]
fn tsc_define_expose_mixed_shorthand_and_complex() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import { ref, computed } from 'vue'
const x = ref(1)
defineExpose({ x, y: computed(() => 2) })
</script><template/>"#,
    );

    assert!(r.contains("typeof x"), "shorthand x should use typeof: {r}");
    assert!(
        r.contains("y: any"),
        "complex y should fall back to any: {r}"
    );
}

#[test]
fn tsc_define_expose_type_param_unchanged() {
    // Regression: type-param form must continue to use the intersection type
    let r = gen_tsc(
        r#"<script setup lang="ts">
const foo = ref(1)
defineExpose<{ foo: number }>({ foo })
</script><template/>"#,
    );

    assert!(
        r.contains("{ foo: number }"),
        "type-param form should use the type text directly: {r}"
    );
    assert!(
        !r.contains("ShallowUnwrapRef"),
        "type-param form should NOT use ShallowUnwrapRef: {r}"
    );
}

#[test]
fn tsc_define_expose_empty_unchanged() {
    // Regression: empty defineExpose should not emit ShallowUnwrapRef
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineExpose()
</script><template/>"#,
    );

    assert!(
        !r.contains("ShallowUnwrapRef"),
        "empty expose should NOT use ShallowUnwrapRef: {r}"
    );
}

#[test]
fn tsc_define_expose_includes_setup_content() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const foo = ref(1)
defineExpose({ foo })
</script><template/>"#,
    );

    // Script body must be present so typeof can resolve
    assert!(
        r.contains("const foo = ref(1)"),
        "output should include the script setup body: {r}"
    );
    // Macro stubs must be present so defineExpose doesn't error
    assert!(
        r.contains("declare function defineExpose"),
        "output should include macro stubs: {r}"
    );
}

// ── Dual-script JS SFC (verter-tsc path) ────────────────────────

#[test]
fn tsc_dual_script_js_sfc_basic() {
    let r = gen_tsc(
        r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>
<script>
export default {
  inheritAttrs: false,
}
</script>
<template><div>{{ count }}</div></template>"#,
    );

    // Should still produce valid TSC output
    assert!(
        r.contains("defineComponent"),
        "should have defineComponent call:\n{r}"
    );
    assert!(
        r.contains("export default"),
        "should have export default:\n{r}"
    );

    // Should NOT contain raw script tags
    assert!(!r.contains("<script"), "script tags must not appear:\n{r}");
    assert!(
        !r.contains("</script>"),
        "close script tags must not appear:\n{r}"
    );

    // Should parse as valid TypeScript
    let alloc = oxc_allocator::Allocator::new();
    let parsed = oxc_parser::Parser::new(&alloc, &r, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.errors {
        eprintln!("OXC TSC ERROR: {err}");
    }
    assert!(
        parsed.errors.is_empty(),
        "generated TSC output should have no parse errors, got {}:\n{r}",
        parsed.errors.len()
    );
}

#[test]
fn tsc_dual_script_js_vuetify_figure_pattern() {
    let r = gen_tsc(
        r#"<template>
  <figure>
    <figcaption v-if="caption" v-text="caption" />
    <slot v-else />
  </figure>
</template>

<script setup>
  import { computed, useAttrs } from 'vue'

  const attrs = useAttrs()

  defineProps({
    name: String,
  })

  const caption = computed(() => attrs.title === 'null' ? null : attrs.title)
</script>

<script>
  export default {
    inheritAttrs: false,
  }
</script>"#,
    );

    // Should generate valid TSC output with props
    assert!(
        r.contains("defineComponent"),
        "should have defineComponent:\n{r}"
    );
    assert!(r.contains("name:"), "should have name prop:\n{r}");

    // Should NOT contain script tags or companion export default content
    assert!(!r.contains("<script"), "script tags must not appear:\n{r}");

    // Should parse as valid TypeScript
    let alloc = oxc_allocator::Allocator::new();
    let parsed = oxc_parser::Parser::new(&alloc, &r, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.errors {
        eprintln!("OXC TSC ERROR: {err}");
    }
    assert!(
        parsed.errors.is_empty(),
        "generated TSC output should have no parse errors, got {}:\n{r}",
        parsed.errors.len()
    );
}

#[test]
fn reserved_word_component_name_is_prefixed() {
    let code = generate_tsc_output_with_options(
        "<template><div>hello</div></template>",
        "default",
        &TscGenOptions::default(),
    )
    .code;
    assert!(
        code.contains("_default"),
        "reserved word 'default' should be prefixed with _, got:\n{}",
        code
    );
    assert!(
        !code.contains("const default"),
        "should not produce `const default` (reserved word), got:\n{}",
        code
    );
}

#[test]
fn digit_prefix_component_name_is_prefixed() {
    let code = generate_tsc_output_with_options(
        "<template><div>not found</div></template>",
        "404",
        &TscGenOptions::default(),
    )
    .code;
    assert!(
        code.contains("_404"),
        "digit-prefixed name should get _ prefix, got:\n{}",
        code
    );
    assert!(
        !code.contains("const 404"),
        "should not produce `const 404` (invalid identifier), got:\n{}",
        code
    );
}
