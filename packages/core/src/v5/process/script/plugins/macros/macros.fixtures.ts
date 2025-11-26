/**
 * Macros Plugin Type Fixtures
 * 
 * This file defines fixtures for testing the macro transformation types.
 * It is used by:
 * - scripts/fixtures.generator.ts: To generate __generated__/*.ts files
 * - macros.types.spec.ts: For automated testing
 * 
 * Run `pnpm generate:fixtures` to regenerate the fixture files.
 */

import { MagicString } from "@vue/compiler-sfc";
import { parser } from "../../../../parser";
import { ParsedBlockScript } from "../../../../parser/types";
import { processScript } from "../../script";
import { MacrosPlugin } from "./index.js";
import { TemplateBindingPlugin } from "../template-binding";
import { ScriptBlockPlugin } from "../script-block";
import { BindingPlugin } from "../binding";
import type { Fixture, FixtureConfig, ProcessResult } from "../../../../../fixtures/types";

/**
 * Process Vue SFC content through the macros pipeline
 */
function processMacros(code: string, lang = "ts"): ProcessResult {
  const prepend = `<script setup lang="${lang}">`;
  const source = `${prepend}${code}</script>`;
  const parsed = parser(source);

  const s = new MagicString(source);

  const scriptBlock = parsed.blocks.find(
    (x) => x.type === "script"
  ) as ParsedBlockScript;

  const result = processScript(
    scriptBlock.result.items,
    [MacrosPlugin, TemplateBindingPlugin, ScriptBlockPlugin, BindingPlugin],
    {
      s,
      filename: "test.vue",
      blocks: parsed.blocks,
      isSetup: true,
      block: scriptBlock,
      blockNameResolver: (name: string) => name,
    }
  );

  return {
    result: result.result,
    context: result.context,
  };
}

/**
 * Fixture definitions for macros
 */
export const fixtures: Fixture[] = [
  {
    name: "defineProps with type argument",
    code: `
const props = defineProps<{
  msg: string;
  count?: number;
  items: string[];
}>();
`,
    expectations: {
      typeAliases: ["___VERTER___defineProps_Type"],
      patterns: [
        "defineProps<___VERTER___defineProps_Type>()",
        "___VERTER___Prettify",
        "msg: string",
        "count?: number",
        "items: string[]",
      ],
    },
  },
  {
    name: "defineProps with object syntax",
    code: `
const props = defineProps({
  msg: String,
  count: { type: Number, default: 0 },
});
`,
    expectations: {
      boxedVariables: ["___VERTER___defineProps_Boxed"],
      patterns: [
        "___VERTER___defineProps_Box(",
        "msg: String",
        "count: { type: Number",
      ],
      antiPatterns: ["___VERTER___defineProps_Type"],
    },
  },
  {
    name: "defineProps with withDefaults",
    code: `
const props = withDefaults(defineProps<{
  msg: string;
  count?: number;
  enabled?: boolean;
}>(), {
  count: 10,
  enabled: true,
});
`,
    expectations: {
      typeAliases: ["___VERTER___defineProps_Type"],
      boxedVariables: ["___VERTER___withDefaults_Boxed"],
      patterns: [
        "___VERTER___withDefaults_Box(",
        "msg: string",
        "count?: number",
        "enabled?: boolean",
      ],
    },
  },
  {
    name: "defineEmits with type argument",
    code: `
const emit = defineEmits<{
  change: [value: string];
  update: [id: number, data: { name: string }];
}>();
`,
    expectations: {
      typeAliases: ["___VERTER___defineEmits_Type"],
      patterns: [
        "defineEmits<___VERTER___defineEmits_Type>()",
        "___VERTER___Prettify",
        "change: [value: string]",
      ],
    },
  },
  {
    name: "defineEmits with array syntax",
    code: `
const emit = defineEmits(['change', 'update']);
`,
    expectations: {
      boxedVariables: ["___VERTER___defineEmits_Boxed"],
      patterns: [
        "___VERTER___defineEmits_Box(",
        "'change'",
        "'update'",
      ],
      antiPatterns: ["___VERTER___defineEmits_Type"],
    },
  },
  {
    name: "defineModel basic",
    code: `
const model = defineModel<string>();
`,
    expectations: {
      typeAliases: ["___VERTER___modelValue_defineModel_Type"],
      patterns: [
        "defineModel<___VERTER___modelValue_defineModel_Type>",
        "___VERTER___Prettify",
      ],
    },
  },
  {
    name: "defineModel with options",
    code: `
const model = defineModel<number>({ default: 0 });
`,
    expectations: {
      typeAliases: ["___VERTER___modelValue_defineModel_Type"],
      boxedVariables: ["___VERTER___modelValue_defineModel_Boxed"],
      patterns: [
        "___VERTER___defineModel_Box(",
        "default: 0",
      ],
    },
  },
  {
    name: "defineModel named",
    code: `
const count = defineModel<number>('count');
const name = defineModel<string>('name', { required: true });
`,
    expectations: {
      typeAliases: [
        "___VERTER___count_defineModel_Type",
        "___VERTER___name_defineModel_Type",
      ],
      boxedVariables: ["___VERTER___name_defineModel_Boxed"],
      patterns: [
        "defineModel<___VERTER___count_defineModel_Type>",
        "defineModel<___VERTER___name_defineModel_Type>",
      ],
    },
  },
  {
    name: "defineSlots",
    code: `
const slots = defineSlots<{
  default: (props: { item: string }) => void;
  header: () => void;
}>();
`,
    expectations: {
      typeAliases: ["___VERTER___defineSlots_Type"],
      patterns: [
        "defineSlots<___VERTER___defineSlots_Type>()",
        "default: (props: { item: string })",
        "header: () => void",
      ],
    },
  },
  {
    name: "defineExpose",
    code: `
const count = ref(0);
const increment = () => count.value++;

defineExpose({
  count,
  increment,
});
`,
    expectations: {
      patterns: ["defineExpose("],
    },
  },
  {
    name: "complex component with all macros",
    code: `
import { ref, computed } from 'vue';

interface Item {
  id: number;
  name: string;
  active: boolean;
}

const props = defineProps<{
  items: Item[];
  title: string;
  maxItems?: number;
}>();

const emit = defineEmits<{
  select: [item: Item];
  delete: [id: number];
  update: [items: Item[]];
}>();

const selected = defineModel<Item | null>('selected');
const filter = defineModel<string>('filter', { default: '' });

const slots = defineSlots<{
  default: (props: { items: Item[] }) => void;
  item: (props: { item: Item; index: number }) => void;
  empty: () => void;
}>();

const filteredItems = computed(() => 
  props.items.filter(item => item.name.includes(filter.value ?? ''))
);

const selectItem = (item: Item) => {
  selected.value = item;
  emit('select', item);
};

defineExpose({
  filteredItems,
  selectItem,
});
`,
    expectations: {
      typeAliases: [
        "___VERTER___defineProps_Type",
        "___VERTER___defineEmits_Type",
        "___VERTER___selected_defineModel_Type",
        "___VERTER___filter_defineModel_Type",
        "___VERTER___defineSlots_Type",
      ],
      patterns: [
        "items: Item[]",
        "title: string",
        "select: [item: Item]",
      ],
    },
  },
  {
    name: "destructured defineProps",
    code: `
const { msg, count = 0 } = defineProps<{
  msg: string;
  count?: number;
}>();
`,
    expectations: {
      typeAliases: ["___VERTER___defineProps_Type"],
      patterns: [
        "defineProps<___VERTER___defineProps_Type",
        "msg: string",
        "count?: number",
      ],
    },
  },
  {
    name: "empty defineProps",
    code: `
const props = defineProps()
`,
    expectations: {
      antiPatterns: ["___VERTER___defineProps_Type"],
    },
  },
  {
    name: "empty defineEmits",
    code: `
const emit = defineEmits()
`,
    expectations: {
      antiPatterns: ["___VERTER___defineEmits_Type"],
    },
  },
];

/**
 * Type inspection fixtures - designed to test what types show on hover
 */
export const typeInspectionFixtures: Fixture[] = [
  {
    name: "props hover shows expanded type",
    code: `
const props = defineProps<{
  name: string;
  age: number;
  active?: boolean;
}>();

// Hover over 'props' should show: { name: string; age: number; active?: boolean }
`,
    expectations: {
      patterns: ["___VERTER___Prettify"],
    },
  },
  {
    name: "emit hover shows event signatures",
    code: `
const emit = defineEmits<{
  submit: [data: { name: string }];
  cancel: [];
}>();

// Hover over 'emit' should show the emit function signature
`,
    expectations: {
      patterns: ["___VERTER___Prettify"],
    },
  },
  {
    name: "model hover shows ModelRef type",
    code: `
const count = defineModel<number>('count');

// Hover over 'count' should show: ModelRef<number, string>
`,
    expectations: {
      typeAliases: ["___VERTER___count_defineModel_Type"],
    },
  },
];

/**
 * Create the fixture configuration for the generator
 */
export function createFixtures(): FixtureConfig {
  return {
    fixtures: [...fixtures, ...typeInspectionFixtures],
    process: processMacros,
    // Use empty prefix for cleaner output
    prefix: "",
  };
}
