<script lang="ts">
// v-model patterns for parser testing - Options API + TypeScript

import { defineComponent, type PropType } from "vue";

// Custom modifiers interface
interface CustomModifiers {
  capitalize?: boolean;
  uppercase?: boolean;
}

export default defineComponent({
  name: "VModelObjectTs",

  props: {
    // Basic v-model (modelValue is the default prop)
    modelValue: {
      type: String as PropType<string>,
      default: "",
    },
    // Named v-model props
    title: {
      type: String as PropType<string>,
      default: "Untitled",
    },
    firstName: {
      type: String as PropType<string>,
      default: "",
    },
    lastName: {
      type: String as PropType<string>,
      default: "",
    },
    // Numeric model
    count: {
      type: Number as PropType<number>,
      required: true,
    },
    // Model with custom modifiers
    custom: {
      type: String as PropType<string>,
      default: "",
    },
    customModifiers: {
      type: Object as PropType<CustomModifiers>,
      default: () => ({}),
    },
  },

  emits: {
    "update:modelValue": (value: string) => true,
    "update:title": (value: string) => true,
    "update:firstName": (value: string) => true,
    "update:lastName": (value: string) => true,
    "update:count": (value: number) => true,
    "update:custom": (value: string) => true,
  },

  data() {
    return {
      // Local state for native inputs
      text: "" as string,
      checked: false as boolean,
      selected: "option1" as string,
      multiSelected: [] as string[],

      // v-model modifiers demo
      lazyText: "" as string,
      numericValue: 0 as number,
      trimmedText: "" as string,
      lazyTrimmed: "" as string,
      lazyNumber: 0 as number,
    };
  },

  computed: {
    // Writable computed for two-way binding on custom props
    localModelValue: {
      get(): string {
        return this.modelValue;
      },
      set(value: string) {
        this.$emit("update:modelValue", value);
      },
    },

    localTitle: {
      get(): string {
        return this.title;
      },
      set(value: string) {
        this.$emit("update:title", value);
      },
    },

    localFirstName: {
      get(): string {
        return this.firstName;
      },
      set(value: string) {
        this.$emit("update:firstName", value);
      },
    },

    localLastName: {
      get(): string {
        return this.lastName;
      },
      set(value: string) {
        this.$emit("update:lastName", value);
      },
    },

    localCount: {
      get(): number {
        return this.count;
      },
      set(value: number) {
        this.$emit("update:count", value);
      },
    },

    localCustom: {
      get(): string {
        return this.custom;
      },
      set(value: string) {
        this.$emit("update:custom", value);
      },
    },

    // Transform based on custom modifiers
    transformedCustom(): string {
      if (!this.custom) return "";
      if (this.customModifiers?.capitalize) {
        return this.custom.charAt(0).toUpperCase() + this.custom.slice(1);
      }
      if (this.customModifiers?.uppercase) {
        return this.custom.toUpperCase();
      }
      return this.custom;
    },
  },
});
</script>

<template>
  <div>
    <!-- Basic v-model on native inputs -->
    <input v-model="text" type="text" />
    <input v-model="checked" type="checkbox" />
    <select v-model="selected">
      <option value="option1">Option 1</option>
      <option value="option2">Option 2</option>
    </select>
    <select v-model="multiSelected" multiple>
      <option value="a">A</option>
      <option value="b">B</option>
    </select>

    <!-- v-model with modifiers -->
    <input v-model.lazy="lazyText" type="text" />
    <input v-model.number="numericValue" type="number" />
    <input v-model.trim="trimmedText" type="text" />

    <!-- Combined modifiers -->
    <input v-model.lazy.trim="lazyTrimmed" type="text" />
    <input v-model.lazy.number="lazyNumber" type="number" />

    <!-- Two-way binding on component props -->
    <input v-model="localModelValue" />
    <input v-model="localTitle" />
    <input v-model="localFirstName" />
    <input v-model="localLastName" />
    <input v-model.number="localCount" />
    <input v-model="localCustom" />
    <span>{{ transformedCustom }}</span>
  </div>
</template>
