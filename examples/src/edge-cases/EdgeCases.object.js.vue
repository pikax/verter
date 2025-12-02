<script>
// Edge case patterns for parser testing - Options API + JavaScript
// This file contains unusual or complex parsing scenarios

import { defineComponent } from "vue";

export default defineComponent({
  name: "EdgeCasesObjectJs",

  props: {
    // Union types (multiple types)
    value: {
      type: [String, Number, Boolean],
      default: "default",
    },
    // Complex object
    config: {
      type: Object,
      default: () => ({ name: "unnamed", id: 0 }),
    },
    // Nested object
    nested: {
      type: Object,
      required: true,
    },
    // Tuple-like array
    tuple: {
      type: Array,
      default: () => ["", 0, false],
    },
    // Array with objects
    items: {
      type: Array,
      default: () => [],
    },
    // String (template literal type approximation not possible in JS)
    eventName: {
      type: String,
      default: "onClick",
    },
  },

  emits: ["update", "change", "select"],

  data() {
    return {
      // Complex data types
      complexMap: new Map(),
      
      // Dynamic key object
      dynamicKey: "computed",
      
      // Template literal in data
      templateLiteral: `
        Multi-line
        string
      `,
      
      // Regex patterns
      regexPattern: /\d+(?:\.\d+)?/g,
    };
  },

  computed: {
    // Complex computed
    processedItems() {
      return this.items.map((item) => ({
        ...item,
        processed: true,
      }));
    },

    // Computed with optional chaining
    deepValue() {
      return this.nested?.level1?.level2?.level3?.value ?? "default";
    },

    // Object with computed property names
    objectWithDynamicKey() {
      return {
        staticKey: 1,
        [this.dynamicKey]: 2,
        [`${this.dynamicKey}2`]: 3,
      };
    },

    // Destructured config
    configName() {
      return this.config?.name ?? "unnamed";
    },
  },

  methods: {
    // Process items with transformer
    processItems(items, transformer) {
      return items.map(transformer);
    },

    // Async method
    async fetchData(endpoint) {
      return { data: {}, metadata: { timestamp: Date.now() } };
    },

    // Destructuring in parameters
    handleEvent({ target: { value } }) {
      console.log(value);
    },

    // Type narrowing (runtime checks)
    narrowType(value) {
      if (typeof value === "string") return value;
      if (typeof value === "number") return String(value);
      return value ? "true" : "false";
    },

    // Spread operations
    getSpreadArray() {
      return [...this.items, { id: "new" }];
    },

    getSpreadObject() {
      return { ...this.config, extra: true };
    },
  },

  watch: {
    // Watch with handler object
    value: {
      handler(newVal, oldVal) {
        console.log("Value changed:", { newVal, oldVal });
      },
      immediate: true,
    },

    // Deep watch
    nested: {
      handler(newVal) {
        console.log("Nested changed:", newVal?.level1?.level2?.level3?.value);
      },
      deep: true,
    },

    // Watch nested property
    "config.name"(newName, oldName) {
      console.log("Config name changed:", { newName, oldName });
    },
  },
});
</script>

<template>
  <div>
    <!-- Complex template expressions -->
    <div>{{ typeof value === 'string' ? value.toUpperCase() : value }}</div>
    
    <!-- Optional chaining in template -->
    <span>{{ nested?.level1?.level2?.level3?.value }}</span>
    
    <!-- Complex ternary -->
    <div :class="value === 'active' 
      ? 'active-class' 
      : value === 'inactive' 
        ? 'inactive-class' 
        : 'default-class'"
    >
      Nested ternary
    </div>
    
    <!-- Template literal in binding -->
    <div :data-info="`Value: ${value}, Name: ${configName}`">
      Template literal attribute
    </div>
    
    <!-- Destructuring in v-for -->
    <div v-for="{ id, ...rest } in items" :key="id">
      {{ id }} - {{ JSON.stringify(rest) }}
    </div>
    
    <!-- Complex computed in template -->
    <ul>
      <li v-for="item in processedItems" :key="item.id">
        {{ item.processed }}
      </li>
    </ul>
    
    <!-- Arrow function in event handler -->
    <button @click="(e) => handleEvent({ target: { value: 'clicked' } })">
      Arrow in handler
    </button>
    
    <!-- Object spread in binding -->
    <div v-bind="{ ...getSpreadObject(), 'data-spread': true }">
      Spread in v-bind
    </div>
    
    <!-- Dynamic property access -->
    <span>{{ objectWithDynamicKey[dynamicKey] }}</span>
  </div>
</template>
