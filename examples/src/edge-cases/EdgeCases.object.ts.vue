<script lang="ts">
// Edge case patterns for parser testing - Options API + TypeScript
// This file contains unusual or complex parsing scenarios

import { defineComponent, type PropType } from "vue";

// Complex type definitions
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

interface ItemType {
  id: string | number;
  [key: string]: unknown;
}

export default defineComponent({
  name: "EdgeCasesObjectTs",

  props: {
    // Union types via PropType
    value: {
      type: [String, Number, Boolean] as PropType<string | number | boolean>,
      default: "default",
    },
    // Complex object type
    config: {
      type: Object as PropType<{ name: string } & { id: number }>,
      default: () => ({ name: "unnamed", id: 0 }),
    },
    // Nested interface
    nested: {
      type: Object as PropType<NestedInterface>,
      required: true,
    },
    // Tuple type
    tuple: {
      type: Array as unknown as PropType<[string, number, boolean]>,
      default: () => ["", 0, false],
    },
    // Array with complex item type
    items: {
      type: Array as PropType<Array<ItemType>>,
      default: () => [],
    },
    // Template literal type approximation
    eventName: {
      type: String as PropType<`on${Capitalize<string>}`>,
      default: "onClick",
    },
  },

  emits: {
    // Typed emit validators
    update: (value: string | number) => true,
    change: (payload: { old: unknown; new: unknown }) => true,
    select: (item: ItemType) => true,
  },

  data() {
    return {
      // Complex data types
      complexMap: new Map<string, Set<number>>() as Map<string, Set<number>>,
      
      // Dynamic key object
      dynamicKey: "computed" as string,
      
      // Template literal in data
      templateLiteral: `
        Multi-line
        string
      ` as string,
      
      // Regex patterns
      regexPattern: /\d+(?:\.\d+)?/g as RegExp,
    };
  },

  computed: {
    // Complex computed with type narrowing
    processedItems(): ReadonlyArray<ItemType & { processed: true }> {
      return this.items.map((item) => ({
        ...item,
        processed: true as const,
      }));
    },

    // Computed with optional chaining
    deepValue(): string {
      return this.nested?.level1?.level2?.level3?.value ?? "default";
    },

    // Object with computed property names
    objectWithDynamicKey(): Record<string, number> {
      return {
        staticKey: 1,
        [this.dynamicKey]: 2,
        [`${this.dynamicKey}2`]: 3,
      };
    },

    // Destructured config
    configName(): string {
      return this.config?.name ?? "unnamed";
    },
  },

  methods: {
    // Generic-like method pattern
    processItems<T extends ItemType>(
      items: T[],
      transformer: (item: T) => T & { transformed: true }
    ): Array<ReturnType<typeof transformer>> {
      return items.map(transformer);
    },

    // Async method with complex return type
    async fetchData<T extends object>(
      endpoint: string
    ): Promise<{ data: T; metadata: { timestamp: number } }> {
      return { data: {} as T, metadata: { timestamp: Date.now() } };
    },

    // Destructuring in parameters
    handleEvent({ target: { value } }: { target: { value: string } }): void {
      console.log(value);
    },

    // Type narrowing method
    narrowType(value: string | number | boolean): string {
      if (typeof value === "string") return value;
      if (typeof value === "number") return String(value);
      return value ? "true" : "false";
    },

    // Spread operations
    getSpreadArray(): Array<ItemType | { id: string }> {
      return [...this.items, { id: "new" }];
    },

    getSpreadObject(): Record<string, unknown> {
      return { ...this.config, extra: true };
    },
  },

  watch: {
    // Complex watch with typed parameters
    value: {
      handler(newVal: string | number | boolean, oldVal: string | number | boolean) {
        console.log("Value changed:", { newVal, oldVal });
      },
      immediate: true,
    },

    // Deep watch on nested object
    nested: {
      handler(newVal: NestedInterface) {
        console.log("Nested changed:", newVal?.level1?.level2?.level3?.value);
      },
      deep: true,
    },

    // Watch multiple sources (via method name)
    "config.name"(newName: string, oldName: string) {
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
