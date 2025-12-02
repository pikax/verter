<script setup lang="ts">
// v-model patterns for parser testing - Setup API + TypeScript

import { ref, computed } from "vue";

// Basic v-model on native inputs
const text = ref<string>("");
const checked = ref<boolean>(false);
const selected = ref<string>("option1");
const multiSelected = ref<string[]>([]);

// v-model modifiers (.lazy, .number, .trim)
const lazyText = ref<string>("");
const numericValue = ref<number>(0);
const trimmedText = ref<string>("");

// Multiple modifiers combined
const lazyTrimmed = ref<string>("");
const lazyNumber = ref<number>(0);

// Custom v-model implementation using defineModel
const modelValue = defineModel<string>();
const modelWithDefault = defineModel<string>("title", { default: "Untitled" });

// Multiple models on component
const firstName = defineModel<string>("firstName");
const lastName = defineModel<string>("lastName");

// Model with modifiers
const modelWithModifiers = defineModel<string>("search", {
  get(value) {
    return value?.toLowerCase() ?? "";
  },
  set(value) {
    return value?.trim() ?? "";
  },
});

// Custom modifiers (note: Vue's defineModel modifiers are Record<string, true | undefined>)
type CustomModifierKeys = "capitalize" | "uppercase";

// Model with custom name and modifiers
const customModel = defineModel<string>("custom");
const customModifiers = defineModifiers<{ capitalize?: boolean; uppercase?: boolean }>();

// Using modifiers - access via the model's modifiers
const transformedCustom = computed(() => {
  const value = customModel.value;
  if (!value) return "";
  if (customModifiers?.capitalize) {
    return value.charAt(0).toUpperCase() + value.slice(1);
  }
  if (customModifiers?.uppercase) {
    return value.toUpperCase();
  }
  return value;
});

// Helper function type for modifiers (actual implementation would be from Vue)
function defineModifiers<T>(): T | undefined {
  // This would be provided by Vue macros in actual usage
  return undefined;
}

// Model with required
const requiredModel = defineModel<number>("count", { required: true });

// Model with default value
const localModel = defineModel<string>("local", { default: "local value" });
</script>

<template>
  <div>
    <!-- Basic v-model -->
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

    <!-- Using defineModel values -->
    <input v-model="modelValue" />
    <input v-model="modelWithDefault" />
    <input v-model="firstName" />
    <input v-model="lastName" />
    <input v-model="modelWithModifiers" />
    <input v-model="customModel" />
    <span>{{ transformedCustom }}</span>
    <input v-model.number="requiredModel" />
    <input v-model="localModel" />
  </div>
</template>
