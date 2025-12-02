<script setup>
// v-model patterns for parser testing - Setup API + JavaScript

import { ref, computed } from "vue";

// Basic v-model on native inputs
const text = ref("");
const checked = ref(false);
const selected = ref("option1");
const multiSelected = ref([]);

// v-model modifiers (.lazy, .number, .trim)
const lazyText = ref("");
const numericValue = ref(0);
const trimmedText = ref("");

// Multiple modifiers combined
const lazyTrimmed = ref("");
const lazyNumber = ref(0);

// Custom v-model implementation using defineModel
const modelValue = defineModel();
const modelWithDefault = defineModel("title", { default: "Untitled" });

// Multiple models on component
const firstName = defineModel("firstName");
const lastName = defineModel("lastName");

// Model with modifiers
const modelWithModifiers = defineModel("search", {
  get(value) {
    return value?.toLowerCase() ?? "";
  },
  set(value) {
    return value?.trim() ?? "";
  },
});

// Custom modifiers
const [customModel, customModifiers] = defineModel("custom");

// Using modifiers
const transformedCustom = computed(() => {
  if (!customModel.value) return "";
  if (customModifiers.capitalize) {
    return customModel.value.charAt(0).toUpperCase() + customModel.value.slice(1);
  }
  if (customModifiers.uppercase) {
    return customModel.value.toUpperCase();
  }
  return customModel.value;
});

// Model with required
const requiredModel = defineModel("count", { required: true });

// Model with local option (one-way binding)
const localModel = defineModel("local", { local: true, default: "local value" });
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
