<script setup>
// Edge case patterns for parser testing - Setup API + JavaScript
// This file contains unusual or complex parsing scenarios

import { ref, computed, watch, watchEffect } from "vue";

// Complex props with various JavaScript types
const props = defineProps({
  // Union-like types (via validator)
  value: {
    type: [String, Number, Boolean],
    default: "default",
  },
  // Object with nested structure
  config: {
    type: Object,
    default: () => ({ name: "unnamed", id: 0 }),
  },
  // Array with object items
  items: {
    type: Array,
    default: () => [],
  },
  // Tuple-like (array with fixed structure)
  tuple: {
    type: Array,
    default: () => ["", 0, false],
  },
  // Nested object
  nested: {
    type: Object,
    default: () => ({
      level1: {
        level2: {
          level3: { value: "" },
        },
      },
    }),
  },
});

// Complex emits
const emit = defineEmits(["update", "change", "select"]);

// Destructuring with defaults
const { 
  value = "default", 
  config: { name: configName = "unnamed" } = { name: "unnamed", id: 0 },
} = props;

// Multiple defineModel calls
const basicModel = defineModel();
const namedModel = defineModel("count", { required: true });
const modelWithTransform = defineModel("search", {
  get(v) { return v?.toUpperCase() ?? ""; },
  set(v) { return v?.toLowerCase() ?? ""; },
});

// Complex computed
const complexComputed = computed(() => {
  const result = props.items.map((item) => ({
    ...item,
    processed: true,
  }));
  return result;
});

// Ref with complex structure
const complexRef = ref(new Map());

// Function with complex logic
function processItems(items, transformer) {
  return items.map(transformer);
}

// Async function
async function fetchData(endpoint) {
  return { data: {}, metadata: { timestamp: Date.now() } };
}

// Watch with complex source
watch(
  () => [props.value, props.config],
  ([newValue, newConfig], [oldValue, oldConfig]) => {
    console.log("Changed:", { newValue, newConfig, oldValue, oldConfig });
  },
  { deep: true, immediate: true }
);

// Slots definition (JS style)
defineSlots();

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
const noParens = x => x * 2;
const withParens = (x) => x * 2;
const multiLine = (x) => {
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
function handleEvent({ target: { value } }) {
  console.log(value);
}

// Optional chaining and nullish coalescing
const deepAccess = props.nested?.level1?.level2?.level3?.value ?? "default";
const nullishAssign = (complexRef.value ??= new Map());

// Spread in various contexts
const spreadArray = [...props.items, { id: "new" }];
const spreadObject = { ...props.config, extra: true };

// Type narrowing patterns (runtime checks)
function narrowType(value) {
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
