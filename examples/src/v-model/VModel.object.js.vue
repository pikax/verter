<script>
// v-model patterns for parser testing - Options API + JavaScript

import { defineComponent } from "vue";

export default defineComponent({
  name: "VModelObjectJs",

  props: {
    // Basic v-model (modelValue is the default prop)
    modelValue: {
      type: String,
      default: "",
    },
    // Named v-model props
    title: {
      type: String,
      default: "Untitled",
    },
    firstName: {
      type: String,
      default: "",
    },
    lastName: {
      type: String,
      default: "",
    },
    // Numeric model
    count: {
      type: Number,
      required: true,
    },
    // Model with custom modifiers
    custom: {
      type: String,
      default: "",
    },
    customModifiers: {
      type: Object,
      default: () => ({}),
    },
  },

  emits: [
    "update:modelValue",
    "update:title",
    "update:firstName",
    "update:lastName",
    "update:count",
    "update:custom",
  ],

  data() {
    return {
      // Local state for native inputs
      text: "",
      checked: false,
      selected: "option1",
      multiSelected: [],

      // v-model modifiers demo
      lazyText: "",
      numericValue: 0,
      trimmedText: "",
      lazyTrimmed: "",
      lazyNumber: 0,
    };
  },

  computed: {
    // Writable computed for two-way binding on custom props
    localModelValue: {
      get() {
        return this.modelValue;
      },
      set(value) {
        this.$emit("update:modelValue", value);
      },
    },

    localTitle: {
      get() {
        return this.title;
      },
      set(value) {
        this.$emit("update:title", value);
      },
    },

    localFirstName: {
      get() {
        return this.firstName;
      },
      set(value) {
        this.$emit("update:firstName", value);
      },
    },

    localLastName: {
      get() {
        return this.lastName;
      },
      set(value) {
        this.$emit("update:lastName", value);
      },
    },

    localCount: {
      get() {
        return this.count;
      },
      set(value) {
        this.$emit("update:count", value);
      },
    },

    localCustom: {
      get() {
        return this.custom;
      },
      set(value) {
        this.$emit("update:custom", value);
      },
    },

    // Transform based on custom modifiers
    transformedCustom() {
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
