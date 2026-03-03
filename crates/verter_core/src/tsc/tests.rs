use super::script::generate_tsc_output;

fn gen_tsc(sfc: &str) -> String {
    generate_tsc_output(sfc, "TestComp").code
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
    assert!(r.contains("$props: Props"), "type name in new()");
    assert!(r.contains("ComponentPublicInstance"), "CPI declaration");
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
        r.contains("'update:title': [v: string]"),
        "model emit type inline"
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
    assert!(r.contains("'update:model': []"), "typed emits in declare");
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
    assert!(r.contains("$props: Props"), "type name in $props");
    assert!(!r.contains("withDefaults"), "macro removed");
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

    assert!(r.contains("modelValue?: User"), "TS User type");
    assert!(
        r.contains("'update:modelValue': [v: User]"),
        "emit type with User"
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
    assert!(r.contains("$props: WatermarkProps"), "type name in $props");
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
        r.contains("$props: Props"),
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
        r.contains("$props: MyProps"),
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
    // $props should reference the type name
    assert!(
        r.contains("$props: Props"),
        "should reference type name in $props: got {}",
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

    // No $slots when defineSlots not used
    assert!(
        !r.contains("$slots"),
        "should not emit $slots without defineSlots: got {}",
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

    // Positive: generic on new()
    assert!(
        r.contains("new<T>()"),
        "should emit generic on new(): got {}",
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
        r.contains("new<T extends string>()"),
        "should emit generic with constraint: got {}",
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
        r.contains("new<K extends string, V>()"),
        "should emit multiple generic params: got {}",
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

    // Without generic, should have plain new()
    assert!(
        r.contains("new(): {"),
        "should have plain new() without generic: got {}",
        r
    );
    assert!(
        !r.contains("new<"),
        "should not have angle brackets without generic: got {}",
        r
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
