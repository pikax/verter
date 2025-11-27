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
import { ImportsPlugin } from "../imports";
import type {
  Fixture,
  FixtureConfig,
  ProcessResult,
} from "../../../../../fixtures/types";

/**
 * Process Vue SFC content through the macros pipeline
 */
function processMacros(
  code: string,
  prefix: string,
  lang = "ts",
  generic?: string
): ProcessResult {
  const genericAttr = generic ? ` generic="${generic}"` : "";
  const prepend = `<script setup lang="${lang}"${genericAttr}>`;
  const source = `${prepend}${code}</script>`;
  const parsed = parser(source);

  const s = new MagicString(source);

  const scriptBlock = parsed.blocks.find(
    (x) => x.type === "script"
  ) as ParsedBlockScript;

  const result = processScript(
    scriptBlock.result.items,
    [
      MacrosPlugin,
      TemplateBindingPlugin,
      ScriptBlockPlugin,
      BindingPlugin,
      ImportsPlugin,
    ],
    {
      s,
      filename: "test.vue",
      blocks: parsed.blocks,
      isSetup: true,
      block: scriptBlock,
      prefix: (name: string) => prefix + name,
      blockNameResolver: (name: string) => name,
    }
  );

  const sourcemap = s
    .generateMap({ hires: true, includeContent: true })
    .toUrl();

  return {
    result: result.result,
    context: result.context,
    sourcemap,
  };
}

/**
 * Fixture definitions for macros
 */
const fixtures: Fixture[] = [
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
      typeAliases: [(p) => p + "defineProps_Type"],
      patterns: [
        (p) => `defineProps<${p}defineProps_Type>()`,
        (p) => p + "Prettify",
        "msg: string",
        "count?: number",
        "items: string[]",
      ],
      typeTests: [
        {
          target: (p) => `${p}defineProps_Type`,
          description: "Props type should contain all defined properties",
          shouldContain: ["msg", "string"],
          notAny: true,
          notUnknown: true,
        },
        {
          target: "props",
          kind: "variable" as const,
          description: "props variable should not be any",
          notAny: true,
          notUnknown: true,
        },
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
      boxedVariables: [(p) => p + "defineProps_Boxed"],
      patterns: [
        (p) => p + "defineProps_Box(",
        "msg: String",
        "count: { type: Number",
      ],
      antiPatterns: [(p) => p + "defineProps_Type"],
      typeTests: [
        {
          target: "props",
          kind: "variable" as const,
          description: "props variable should not be any (object syntax)",
          notAny: true,
          notUnknown: true,
        },
      ],
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
      typeAliases: [(p) => p + "defineProps_Type"],
      boxedVariables: [(p) => p + "withDefaults_Boxed"],
      patterns: [
        (p) => p + "withDefaults_Box(",
        "msg: string",
        "count?: number",
        "enabled?: boolean",
      ],
      typeTests: [
        {
          target: (p) => `${p}defineProps_Type`,
          description:
            "Props type should contain all properties including optional",
          shouldContain: ["msg", "count", "enabled"],
          notAny: true,
        },
        {
          target: "props",
          kind: "variable" as const,
          description: "props variable should have correct type with defaults",
          shouldContain: ["msg"],
          notAny: true,
          notUnknown: true,
        },
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
      typeAliases: [(p) => p + "defineEmits_Type"],
      patterns: [
        (p) => `defineEmits<${p}defineEmits_Type>()`,
        (p) => p + "Prettify",
        "change: [value: string]",
      ],
      typeTests: [
        {
          target: (p) => `${p}defineEmits_Type`,
          description: "Emits type should contain event signatures",
          shouldContain: ["change", "update"],
          notAny: true,
        },
        {
          target: "emit",
          kind: "variable" as const,
          description: "emit function should have correct call signatures",
          // The emit function should be a callable with the event signatures
          shouldContain: ["change", "update"],
          shouldNotContain: ["any"],
          notAny: true,
          notUnknown: true,
        },
      ],
    },
  },
  {
    name: "defineEmits with array syntax",
    code: `
const emit = defineEmits(['change', 'update']);
`,
    expectations: {
      boxedVariables: [(p) => p + "defineEmits_Boxed"],
      patterns: [(p) => p + "defineEmits_Box(", "'change'", "'update'"],
      antiPatterns: [(p) => p + "defineEmits_Type"],
      typeTests: [
        {
          target: "emit",
          kind: "variable" as const,
          description: "emit function should be callable (array syntax)",
          notAny: true,
          notUnknown: true,
        },
      ],
    },
  },
  {
    name: "defineModel basic",
    code: `
const model = defineModel<string>();
`,
    expectations: {
      typeAliases: [(p) => p + "modelValue_defineModel_Type"],
      patterns: [
        (p) => `defineModel<${p}modelValue_defineModel_Type>`,
        (p) => p + "Prettify",
      ],
      typeTests: [
        {
          target: (p) => `${p}modelValue_defineModel_Type`,
          description: "Model type should be string wrapped in Prettify",
          shouldContain: ["string"],
          notAny: true,
        },
        {
          target: "model",
          kind: "variable" as const,
          description: "model variable should not be any",
          notAny: true,
        },
      ],
    },
  },
  {
    name: "defineModel with options",
    code: `
const model = defineModel<number>({ default: 0 });
`,
    expectations: {
      typeAliases: [(p) => p + "modelValue_defineModel_Type"],
      boxedVariables: [(p) => p + "modelValue_defineModel_Boxed"],
      patterns: [(p) => p + "defineModel_Box<number>(", "default: 0"],
      typeTests: [
        {
          target: (p) => `${p}modelValue_defineModel_Type`,
          description: "Model type should be number",
          shouldContain: ["number"],
          notAny: true,
        },
        {
          target: "model",
          kind: "variable" as const,
          description: "model variable should be ModelRef type",
          notAny: true,
          notUnknown: true,
        },
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
        (p) => p + "count_defineModel_Type",
        (p) => p + "name_defineModel_Type",
      ],
      boxedVariables: [(p) => p + "name_defineModel_Boxed"],
      patterns: [
        (p) => `defineModel<${p}count_defineModel_Type>`,
        (p) => `defineModel<${p}name_defineModel_Type>`,
      ],
      typeTests: [
        {
          target: (p) => `${p}count_defineModel_Type`,
          description: "count model type should be number",
          shouldContain: ["number"],
          notAny: true,
        },
        {
          target: (p) => `${p}name_defineModel_Type`,
          description: "name model type should be string",
          shouldContain: ["string"],
          notAny: true,
        },
        {
          target: "count",
          kind: "variable" as const,
          description: "count variable should be ModelRef",
          notAny: true,
          notUnknown: true,
        },
        {
          target: "name",
          kind: "variable" as const,
          description: "name variable should be ModelRef",
          notAny: true,
          notUnknown: true,
        },
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
      typeAliases: [(p) => p + "defineSlots_Type"],
      patterns: [
        (p) => `defineSlots<${p}defineSlots_Type>()`,
        "default: (props: { item: string })",
        "header: () => void",
      ],
      typeTests: [
        {
          target: (p) => `${p}defineSlots_Type`,
          description: "Slots type should contain slot definitions",
          shouldContain: ["default", "header"],
          notAny: true,
        },
        {
          target: "slots",
          kind: "variable" as const,
          description: "slots variable should have correct type",
          shouldContain: ["default", "header"],
          notAny: true,
          notUnknown: true,
        },
      ],
    },
  },
  {
    name: "defineExpose",
    code: `
import { ref } from 'vue';

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
        (p) => p + "defineProps_Type",
        (p) => p + "defineEmits_Type",
        (p) => p + "selected_defineModel_Type",
        (p) => p + "filter_defineModel_Type",
        (p) => p + "defineSlots_Type",
      ],
      patterns: ["items: Item[]", "title: string", "select: [item: Item]"],
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
      typeAliases: [(p) => p + "defineProps_Type"],
      patterns: [
        (p) => `defineProps<${p}defineProps_Type`,
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
      antiPatterns: [(p) => p + "defineProps_Type"],
    },
  },
  {
    name: "empty defineEmits",
    code: `
const emit = defineEmits()
`,
    expectations: {
      antiPatterns: [(p) => p + "defineEmits_Type"],
    },
  },
];

/**
 * Type inspection fixtures - designed to test what types show on hover
 * and validate the structure for user experience
 */
const typeInspectionFixtures: Fixture[] = [
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
      typeAliases: [(p) => p + "defineProps_Type"],
      patterns: [(p) => p + "Prettify"],
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
      typeAliases: [(p) => p + "defineEmits_Type"],
      patterns: [(p) => p + "Prettify"],
    },
  },
  {
    name: "model hover shows ModelRef type",
    code: `
  const count = defineModel<number>('count');

  // Hover over 'count' should show: ModelRef<number, string>
  `,
    expectations: {
      typeAliases: [(p) => p + "count_defineModel_Type"],
      patterns: [(p) => `defineModel<${p}count_defineModel_Type>`],
    },
  },
  {
    name: "withDefaults preserves prop types",
    code: `
  const props = withDefaults(defineProps<{
    msg: string;
    count?: number;
    items?: string[];
  }>(), {
    count: 0,
    items: () => [],
  });

  // Hover over 'props' should show the correct prop types with defaults applied
  `,
    expectations: {
      typeAliases: [(p) => p + "defineProps_Type"],
      boxedVariables: [(p) => p + "withDefaults_Boxed"],
      patterns: ["msg: string", "count?: number", "items?: string[]"],
    },
  },
  {
    name: "multiple models with distinct types",
    code: `
  const firstName = defineModel<string>('firstName');
  const lastName = defineModel<string>('lastName');
  const age = defineModel<number>('age');
  const isActive = defineModel<boolean>('isActive');

  // Each model should have a distinct type alias
  `,
    expectations: {
      typeAliases: [
        (p) => p + "firstName_defineModel_Type",
        (p) => p + "lastName_defineModel_Type",
        (p) => p + "age_defineModel_Type",
        (p) => p + "isActive_defineModel_Type",
      ],
    },
  },
  {
    name: "complex nested prop types",
    code: `
  interface Address {
    street: string;
    city: string;
    zipCode: string;
  }

  interface User {
    id: number;
    name: string;
    email: string;
    address: Address;
    roles: string[];
  }

  const props = defineProps<{
    user: User;
    users: User[];
    selectedId?: number;
    onSelect?: (user: User) => void;
  }>();

  // Hover should show nested types expanded
  `,
    expectations: {
      typeAliases: [(p) => p + "defineProps_Type"],
      patterns: ["user: User", "users: User[]", "selectedId?: number"],
    },
  },
  {
    name: "emit with complex event payloads",
    code: `
  interface FormData {
    name: string;
    email: string;
  }

  const emit = defineEmits<{
    submit: [data: FormData, options?: { validate: boolean }];
    change: [field: keyof FormData, value: string];
    reset: [];
  }>();

  // Emit function should show proper overloads
  `,
    expectations: {
      typeAliases: [(p) => p + "defineEmits_Type"],
      patterns: ["submit:", "change:", "reset:"],
    },
  },
  {
    name: "slots with typed props",
    code: `
  interface Item {
    id: number;
    name: string;
  }

  const slots = defineSlots<{
    default: (props: { items: Item[] }) => any;
    item: (props: { item: Item; index: number }) => any;
    empty: (props: { message: string }) => any;
    header: () => any;
  }>();

  // Slots should show typed slot props
  `,
    expectations: {
      typeAliases: [(p) => p + "defineSlots_Type"],
      patterns: ["default:", "item:", "empty:", "header:"],
    },
  },
  {
    name: "model with required option",
    code: `
  const value = defineModel<string>({ required: true });
  const count = defineModel<number>('count', { required: true, default: 0 });

  // Required models should be reflected in types
  `,
    expectations: {
      typeAliases: [
        (p) => p + "modelValue_defineModel_Type",
        (p) => p + "count_defineModel_Type",
      ],
      boxedVariables: [
        (p) => p + "modelValue_defineModel_Boxed",
        (p) => p + "count_defineModel_Boxed",
      ],
      patterns: ["required: true"],
    },
  },
  {
    name: "object props with validators",
    code: `
  import type { PropType } from 'vue';

  const props = defineProps({
    name: {
      type: String,
      required: true,
      validator: (value) => typeof value === 'string' && value.length > 0,
    },
    age: {
      type: Number,
      default: 0,
      validator: (value) => typeof value === 'number' && value >= 0,
    },
    status: {
      type: String as PropType<'active' | 'inactive'>,
      default: 'active',
    },
  });

  // Object syntax props should be boxed correctly
  `,
    expectations: {
      boxedVariables: [(p) => p + "defineProps_Boxed"],
      patterns: ["type: String", "required: true", "type: Number"],
      antiPatterns: [(p) => p + "defineProps_Type"],
    },
  },
  // ============================================================================
  // Additional coverage for all Vue macro use cases
  // ============================================================================

  // defineProps additional patterns
  {
    name: "defineProps with imported type reference",
    code: `
  // Simulating imported type - in real usage this would be: import type { User } from './types';
  interface User {
    id: number;
    name: string;
    email: string;
  }

  const props = defineProps<{
    user: User;
    users: User[];
  }>();
  `,
    expectations: {
      typeAliases: [(p) => p + "defineProps_Type"],
      patterns: ["user: User", "users: User[]"],
    },
  },
  {
    name: "defineProps with Boolean type (shorthand)",
    code: `
  const props = defineProps({
    disabled: Boolean,
    visible: Boolean,
  });
  `,
    expectations: {
      boxedVariables: [(p) => p + "defineProps_Boxed"],
      patterns: ["disabled: Boolean", "visible: Boolean"],
    },
  },
  {
    name: "defineProps with Array type constructor",
    code: `
  import type { PropType } from 'vue';

  const props = defineProps({
    items: Array,
    tags: { type: Array as PropType<string[]>, default: () => [] },
  });
  `,
    expectations: {
      boxedVariables: [(p) => p + "defineProps_Boxed"],
      patterns: ["items: Array", "type: Array"],
    },
  },
  {
    name: "defineProps with Object type constructor",
    code: `
  import type { PropType } from 'vue';

  const props = defineProps({
    config: Object,
    options: { type: Object as PropType<{ debug: boolean }>, required: true },
  });
  `,
    expectations: {
      boxedVariables: [(p) => p + "defineProps_Boxed"],
      patterns: ["config: Object", "type: Object"],
    },
  },
  {
    name: "defineProps with Function type",
    code: `
  import type { PropType } from 'vue';

  const props = defineProps({
    onClick: Function,
    onSubmit: { type: Function as PropType<(data: any) => void> },
  });
  `,
    expectations: {
      boxedVariables: [(p) => p + "defineProps_Boxed"],
      patterns: ["onClick: Function", "type: Function"],
    },
  },
  {
    name: "defineProps with multiple type constructors",
    code: `
  const props = defineProps({
    value: [String, Number],
    data: { type: [String, Number, Boolean], default: '' },
  });
  `,
    expectations: {
      boxedVariables: [(p) => p + "defineProps_Boxed"],
      patterns: ["[String, Number]", "[String, Number, Boolean]"],
    },
  },

  // defineEmits additional patterns
  {
    name: "defineEmits with call signature syntax",
    code: `
const emit = defineEmits<{
  (e: 'change', id: number): void;
  (e: 'update', value: string): void;
}>();
`,
    expectations: {
      typeAliases: [(p) => p + "defineEmits_Type"],
      patterns: ["(e: 'change'", "(e: 'update'"],
      // Type tests skipped: Known issue where call signature emit types resolve to {}
      // instead of a proper interface containing the event names
    },
  },
  {
    name: "defineEmits with object validation syntax",
    code: `
  const emit = defineEmits({
    change: (id: number) => typeof id === 'number',
    update: (value: string) => typeof value === 'string',
    click: null,
  });
  `,
    expectations: {
      boxedVariables: [(p) => p + "defineEmits_Boxed"],
      patterns: ["change:", "update:", "click: null"],
    },
  },
  {
    name: "defineEmits with no payload events",
    code: `
  const emit = defineEmits<{
    close: [];
    open: [];
    toggle: [];
  }>();
  `,
    expectations: {
      typeAliases: [(p) => p + "defineEmits_Type"],
      patterns: ["close: []", "open: []", "toggle: []"],
    },
  },

  // defineModel additional patterns
  {
    name: "defineModel without type (inferred)",
    code: `
  const model = defineModel();
  `,
    expectations: {
      patterns: ["defineModel()"],
    },
  },
  {
    name: "defineModel with type option instead of generic",
    code: `
  const model = defineModel({ type: String });
  const count = defineModel('count', { type: Number, default: 0 });
  `,
    expectations: {
      boxedVariables: [
        (p) => p + "modelValue_defineModel_Boxed",
        (p) => p + "count_defineModel_Boxed",
      ],
      patterns: ["type: String", "type: Number"],
    },
  },
  {
    name: "defineModel with get/set transformers",
    code: `
  const model = defineModel<string>({
    get(value) {
      return value?.toUpperCase() ?? '';
    },
    set(value) {
      return value.toLowerCase();
    },
  });
  `,
    expectations: {
      typeAliases: [(p) => p + "modelValue_defineModel_Type"],
      boxedVariables: [(p) => p + "modelValue_defineModel_Boxed"],
      patterns: ["get(value)", "set(value)"],
    },
  },
  {
    name: "defineModel with modifiers type",
    code: `
  const [modelValue, modifiers] = defineModel<string, 'trim' | 'uppercase'>();
  `,
    expectations: {
      typeAliases: [(p) => p + "modelValue_defineModel_Type"],
      patterns: ["'trim' | 'uppercase'"],
    },
  },
  {
    name: "defineModel destructured for modifiers",
    code: `
  const [modelValue, modelModifiers] = defineModel();
  `,
    expectations: {
      patterns: ["defineModel()"],
    },
  },

  // defineExpose additional patterns
  {
    name: "defineExpose with type argument",
    code: `
  defineExpose<{
    focus: () => void;
    reset: () => void;
    value: string;
  }>();
  `,
    expectations: {
      typeAliases: [(p) => p + "defineExpose_Type"],
      patterns: ["focus:", "reset:", "value:"],
    },
  },
  {
    name: "defineExpose with computed and refs",
    code: `
  import { ref, computed } from 'vue';

  const count = ref(0);
  const doubled = computed(() => count.value * 2);
  const increment = () => { count.value++ };

  defineExpose({
    count,
    doubled,
    increment,
  });
  `,
    expectations: {
      patterns: ["count,", "doubled,", "increment,"],
    },
  },
  {
    name: "defineExpose empty",
    code: `
  defineExpose();
  `,
    expectations: {
      patterns: ["defineExpose()"],
    },
  },

  // defineOptions patterns
  {
    name: "defineOptions with inheritAttrs",
    code: `
  defineOptions({
    inheritAttrs: false,
  });
  `,
    expectations: {
      patterns: ["inheritAttrs: false"],
    },
  },
  {
    name: "defineOptions with custom options",
    code: `
  defineOptions({
    inheritAttrs: false,
    customOptions: {
      foo: 'bar',
    },
  });
  `,
    expectations: {
      patterns: ["inheritAttrs: false", "customOptions:"],
    },
  },
  {
    name: "defineOptions with name",
    code: `
  defineOptions({
    name: 'MyComponent',
  });
  `,
    expectations: {
      patterns: ["name: 'MyComponent'"],
    },
  },

  // defineSlots additional patterns
  {
    name: "defineSlots with scoped slot props",
    code: `
  const slots = defineSlots<{
    default(props: { msg: string }): any;
    item(props: { item: { id: number; name: string }; index: number }): any;
  }>();
  `,
    expectations: {
      typeAliases: [(p) => p + "defineSlots_Type"],
      patterns: ["default(props:", "item(props:"],
    },
  },
  {
    name: "defineSlots empty",
    code: `
  const slots = defineSlots();
  `,
    expectations: {
      patterns: ["defineSlots()"],
    },
  },

  // withDefaults additional patterns
  {
    name: "withDefaults with factory functions for arrays",
    code: `
  const props = withDefaults(defineProps<{
    items?: string[];
    tags?: number[];
  }>(), {
    items: () => ['default'],
    tags: () => [1, 2, 3],
  });
  `,
    expectations: {
      typeAliases: [(p) => p + "defineProps_Type"],
      boxedVariables: [(p) => p + "withDefaults_Boxed"],
      patterns: ["items: () =>", "tags: () =>"],
    },
  },
  {
    name: "withDefaults with factory functions for objects",
    code: `
  const props = withDefaults(defineProps<{
    config?: { debug: boolean };
    options?: Record<string, any>;
  }>(), {
    config: () => ({ debug: false }),
    options: () => ({}),
  });
  `,
    expectations: {
      typeAliases: [(p) => p + "defineProps_Type"],
      boxedVariables: [(p) => p + "withDefaults_Boxed"],
      patterns: ["config: () =>", "options: () =>"],
    },
  },
  {
    name: "withDefaults with all required props (no defaults needed)",
    code: `
  const props = withDefaults(defineProps<{
    msg: string;
    count: number;
  }>(), {});
  `,
    expectations: {
      typeAliases: [(p) => p + "defineProps_Type"],
      patterns: ["msg: string", "count: number"],
    },
  },

  // Edge cases and combinations
  {
    name: "all macros without assignment",
    code: `
  defineProps<{ msg: string }>();
  defineEmits<{ change: [] }>();
  defineSlots<{ default: () => void }>();
  defineExpose({ foo: 1 });
  defineOptions({ inheritAttrs: false });
  `,
    expectations: {
      patterns: [
        "defineProps<",
        "defineEmits<",
        "defineSlots<",
        "defineExpose(",
        "defineOptions(",
      ],
      typeTests: [
        {
          target: (p) => `${p}props`,
          kind: "variable" as const,
          description: "props should have msg property",
          shouldContain: ["msg"],
          notAny: true,
        },
        {
          target: (p) => `${p}emits`,
          kind: "variable" as const,
          description: "emits should be callable with change event",
          shouldContain: ["change"],
          notAny: true,
        },
        {
          target: (p) => `${p}slots`,
          kind: "variable" as const,
          description: "slots should have default slot",
          shouldContain: ["default"],
          notAny: true,
        },
      ],
    },
  },
  {
    name: "props with literal types",
    code: `
  const props = defineProps<{
    size: 'small' | 'medium' | 'large';
    variant: 'primary' | 'secondary' | 'danger';
    count: 1 | 2 | 3;
  }>();
  `,
    expectations: {
      typeAliases: [(p) => p + "defineProps_Type"],
      patterns: [
        "'small' | 'medium' | 'large'",
        "'primary' | 'secondary' | 'danger'",
      ],
    },
  },
  {
    name: "props with readonly and Readonly utility",
    code: `
  const props = defineProps<{
    readonly items: string[];
    config: Readonly<{ debug: boolean }>;
  }>();
  `,
    expectations: {
      typeAliases: [(p) => p + "defineProps_Type"],
      patterns: ["readonly items:", "Readonly<"],
    },
  },
  {
    name: "emits with complex tuple types",
    code: `
  const emit = defineEmits<{
    update: [key: string, value: unknown, options?: { silent: boolean }];
    batch: [...items: string[]];
  }>();
  `,
    expectations: {
      typeAliases: [(p) => p + "defineEmits_Type"],
      patterns: ["key: string", "options?:"],
      typeTests: [
        {
          target: (p) => `${p}defineEmits_Type`,
          description: "Emits type should contain complex tuple signatures",
          shouldContain: ["update", "batch"],
          notAny: true,
        },
        {
          target: "emit",
          kind: "variable" as const,
          description:
            "emit function should have complex tuple call signatures",
          shouldContain: ["update", "batch"],
          notAny: true,
          notUnknown: true,
        },
      ],
    },
  },
  {
    name: "model with union type",
    code: `
  const model = defineModel<string | number | null>();
  const selected = defineModel<'a' | 'b' | 'c'>('selected');
  `,
    expectations: {
      typeAliases: [
        (p) => p + "modelValue_defineModel_Type",
        (p) => p + "selected_defineModel_Type",
      ],
      patterns: ["string | number | null", "'a' | 'b' | 'c'"],
    },
  },
  {
    name: "props with generic constraints",
    code: `
  interface BaseItem {
    id: number;
  }

  const props = defineProps<{
    item: BaseItem;
    items: Array<BaseItem>;
    map: Map<string, BaseItem>;
    set: Set<BaseItem>;
  }>();
  `,
    expectations: {
      typeAliases: [(p) => p + "defineProps_Type"],
      patterns: ["Array<BaseItem>", "Map<string, BaseItem>", "Set<BaseItem>"],
    },
  },
  {
    name: "defineModel default modelValue (no name)",
    code: `
  const modelValue = defineModel<string>();
  `,
    expectations: {
      typeAliases: [(p) => p + "modelValue_defineModel_Type"],
      patterns: [(p) => `defineModel<${p}modelValue_defineModel_Type>`],
    },
  },
  {
    name: "multiple defineModel with same type different names",
    code: `
  const start = defineModel<Date>('start');
  const end = defineModel<Date>('end');
  `,
    expectations: {
      typeAliases: [
        (p) => p + "start_defineModel_Type",
        (p) => p + "end_defineModel_Type",
      ],
    },
  },
  {
    name: "props with index signature",
    code: `
  const props = defineProps<{
    [key: string]: unknown;
    name: string;
  }>();
  `,
    expectations: {
      typeAliases: [(p) => p + "defineProps_Type"],
      patterns: ["[key: string]: unknown", "name: string"],
    },
  },
  {
    name: "emits with native event names",
    code: `
  const emit = defineEmits<{
    click: [event: MouseEvent];
    keydown: [event: KeyboardEvent];
    focus: [event: FocusEvent];
  }>();
  `,
    expectations: {
      typeAliases: [(p) => p + "defineEmits_Type"],
      patterns: ["click:", "keydown:", "focus:"],
    },
  },
  {
    name: "defineProps with conditional/mapped types",
    code: `
  type OptionalProps<T> = {
    [K in keyof T]?: T[K];
  };

  interface Config {
    debug: boolean;
    verbose: boolean;
  }

  const props = defineProps<{
    config: OptionalProps<Config>;
  }>();
  `,
    expectations: {
      typeAliases: [(p) => p + "defineProps_Type"],
      patterns: ["config: OptionalProps<Config>"],
    },
  },

  // ============================================================================
  // Generic component fixtures
  // ============================================================================
  {
    name: "generic component with all macros",
    generic: "T extends { id: number }, K extends string = string",
    code: `
// Generic component that demonstrates all macros with generics
// Note: defineOptions cannot reference generic types as it's hoisted outside the template function

const props = defineProps<{
  items: T[];
  selected: T | null;
  labelKey: K;
}>();

const emit = defineEmits<{
  select: [item: T];
  update: [items: T[]];
  label: [key: K, value: string];
}>();

const model = defineModel<T | null>();
const filter = defineModel<K>('filter');

const slots = defineSlots<{
  default: (props: { items: T[] }) => any;
  item: (props: { item: T; index: number }) => any;
  label: (props: { key: K }) => any;
}>();

// defineOptions works but cannot reference T or K
defineOptions({
  name: 'GenericListComponent',
  inheritAttrs: false,
});

defineExpose({
  items: props.items,
  selectedItem: model,
});
`,
    expectations: {
      typeAliases: [
        (p) => p + "defineProps_Type",
        (p) => p + "defineEmits_Type",
        (p) => p + "modelValue_defineModel_Type",
        (p) => p + "filter_defineModel_Type",
        (p) => p + "defineSlots_Type",
      ],
      patterns: [
        "items: T[]",
        "selected: T | null",
        "labelKey: K",
        "select: [item: T]",
      ],
      typeTests: [
        {
          target: (p) => `${p}defineProps_Type`,
          description: "Props type should preserve generic type parameters",
          shouldContain: ["T", "K"],
          notAny: true,
        },
        {
          target: "props",
          kind: "variable" as const,
          description: "props variable should not be any with generics",
          notAny: true,
          notUnknown: true,
        },
      ],
    },
  },
  {
    name: "generic component simple",
    generic: "T",
    code: `
const props = defineProps<{ item: T }>();
const emit = defineEmits<{ select: [item: T] }>();
`,
    expectations: {
      typeAliases: [
        (p) => p + "defineProps_Type",
        (p) => p + "defineEmits_Type",
      ],
      patterns: ["item: T"],
    },
  },
  {
    name: "generic component with constraints",
    generic: "T extends object",
    code: `
const props = defineProps<{ data: T; keys: (keyof T)[] }>();
`,
    expectations: {
      typeAliases: [(p) => p + "defineProps_Type"],
      patterns: ["data: T", "keyof T"],
    },
  },
  {
    name: "generic component multiple type parameters",
    generic: "T, K extends keyof T, V = T[K]",
    code: `
const props = defineProps<{
  item: T;
  key: K;
  value: V;
}>();
`,
    expectations: {
      typeAliases: [(p) => p + "defineProps_Type"],
      patterns: ["item: T", "key: K", "value: V"],
    },
  },
];

/**
 * Create the fixture configuration for the generator
 */
export function createFixtures(prefix: string = ""): FixtureConfig {
  return {
    fixtures: [...fixtures, ...typeInspectionFixtures],
    process: (code, lang, generic) =>
      processMacros(code, prefix, lang, generic),
    // Use empty prefix for cleaner output
    prefix,
  };
}
