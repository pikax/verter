<script setup lang="ts">
// Edge case patterns for parser testing - Setup API + TypeScript
// This file contains unusual or complex parsing scenarios

import { ref, computed, type Ref, type ComputedRef } from "vue";

// Complex type annotations
type ComplexType<T extends object> = {
  [K in keyof T]: T[K] extends Function ? never : T[K];
};

interface NestedInterface {
  level1: {
    level2: {
      level3: {
        value: string;
      };
    };
  };
}

// Union and intersection types in props
const props = defineProps<{
  // Union types
  value: string | number | boolean;
  // Intersection types
  config: { name: string } & { id: number };
  // Conditional types
  conditional?: ComplexType<{ fn: () => void; data: string }>;
  // Mapped type reference
  nested: NestedInterface;
  // Tuple types
  tuple: [string, number, boolean];
  // Template literal types
  eventName: `on${Capitalize<string>}`;
  // Generic constraints
  items: Array<{ id: string | number }>;
}>();

// Complex emits with overloaded signatures
const emit = defineEmits<{
  // Function overloads
  (e: "update", value: string): void;
  (e: "update", value: number): void;
  (e: "change", payload: { old: unknown; new: unknown }): void;
  // Generic-like pattern
  (e: "select", item: (typeof props.items)[number]): void;
}>();

// Destructuring with complex defaults
const { 
  value = "default", 
  config: { name: configName = "unnamed" } = { name: "unnamed", id: 0 },
} = props;

// Multiple defineModel calls
const basicModel = defineModel<string>();
const namedModel = defineModel<number>("count", { required: true });
const modelWithTransform = defineModel<string>("search", {
  get(v) { return v?.toUpperCase() ?? ""; },
  set(v) { return v?.toLowerCase() ?? ""; },
});

// Complex computed with type assertion
const complexComputed = computed(() => {
  const result = props.items.map((item) => ({
    ...item,
    processed: true as const,
  }));
  return result as ReadonlyArray<typeof result[number]>;
});

// Ref with explicit generic
const explicitRef: Ref<Map<string, Set<number>>> = ref(new Map());

// ComputedRef with explicit type
const explicitComputed: ComputedRef<readonly string[]> = computed(() => 
  props.items.map((i) => String(i.id))
);

// Function with complex generic signature
function processItems<T extends { id: string | number }>(
  items: T[],
  transformer: <U extends T>(item: U) => U & { transformed: true }
): Array<ReturnType<typeof transformer>> {
  return items.map(transformer);
}

// Async function with complex return type
async function fetchData<T extends object>(
  endpoint: string
): Promise<{ data: T; metadata: { timestamp: number } }> {
  return { data: {} as T, metadata: { timestamp: Date.now() } };
}

// Watch with complex source
import { watch, watchEffect } from "vue";

watch(
  () => [props.value, props.config] as const,
  (newVal, oldVal) => {
    // Destructure inside to avoid tuple type issues
    if (newVal && oldVal) {
      const [newValue, newConfig] = newVal;
      const [oldValue, oldConfig] = oldVal;
      console.log("Changed:", { newValue, newConfig, oldValue, oldConfig });
    }
  },
  { deep: true, immediate: true }
);

// Complex slots definition
defineSlots<{
  default(props: { item: unknown; index: number }): any;
  header(props: { title: string }): any;
  footer: () => any;
  // Conditional slot type
  [key: `item-${number}`]: (props: { data: unknown }) => any;
}>();

// Template string expressions
const templateLiteral = ref(`
  Multi-line
  string with ${props.value}
  interpolation
`);

// Regex patterns (tricky to parse)
const regexPattern = /\d+(?:\.\d+)?/g;
const complexRegex = new RegExp(`^${configName}\\s+\\d+$`, "i");

// Arrow functions with various syntaxes
const noParens = (x: number) => x * 2;
const withParens = (x: number): number => x * 2;
const multiLine = (x: number): number => {
  const doubled = x * 2;
  return doubled;
};

// Object shorthand and computed property names
const dynamicKey = "computed";
const objectWithDynamicKey = {
  staticKey: 1,
  [dynamicKey]: 2,
  [`${dynamicKey}2`]: 3,
  [Symbol.iterator]: function* () { yield 1; },
};

// Destructuring in parameters
function handleEvent({ target: { value } }: { target: { value: string } }): void {
  console.log(value);
}

// Optional chaining and nullish coalescing edge cases
const deepAccess = props.nested?.level1?.level2?.level3?.value ?? "default";
const nullishAssign = (explicitRef.value ??= new Map());

// Spread in various contexts
const spreadArray = [...props.items, { id: "new" }];
// Note: HTML id attribute must be string, so we convert it
const spreadObject = { ...props.config, id: String(props.config.id), extra: true };

// Type narrowing patterns
function narrowType(value: string | number | boolean): string {
  if (typeof value === "string") return value;
  if (typeof value === "number") return String(value);
  return value ? "true" : "false";
}
</script>

<template>
  <div>
    <!-- Complex template expressions -->
    <div>{{ typeof props.value === 'string' ? props.value.toUpperCase() : props.value }}</div>
    
    <!-- Optional chaining in template -->
    <span>{{ props.nested?.level1?.level2?.level3?.value }}</span>
    
    <!-- Nullish coalescing in template -->
    <span>{{ basicModel ?? 'No model value' }}</span>
    
    <!-- Complex ternary -->
    <div :class="props.value === 'active' 
      ? 'active-class' 
      : props.value === 'inactive' 
        ? 'inactive-class' 
        : 'default-class'"
    >
      Nested ternary
    </div>
    
    <!-- Template literal in binding -->
    <div :data-info="`Value: ${props.value}, Name: ${configName}`">
      Template literal attribute
    </div>
    
    <!-- Destructuring in v-for -->
    <div v-for="{ id, ...rest } in props.items" :key="id">
      {{ id }} - {{ JSON.stringify(rest) }}
    </div>
    
    <!-- Index signature slot usage -->
    <slot name="item-0" :data="props.items[0]" />
    <slot name="item-1" :data="props.items[1]" />
    
    <!-- Complex computed in template -->
    <ul>
      <li v-for="item in complexComputed" :key="item.id">
        {{ item.processed }}
      </li>
    </ul>
    
    <!-- Arrow function in event handler -->
    <button @click="(e) => handleEvent({ target: { value: 'clicked' } })">
      Arrow in handler
    </button>
    
    <!-- Object spread in binding -->
    <div v-bind="{ ...spreadObject, 'data-spread': true }">
      Spread in v-bind
    </div>
  </div>
</template>
