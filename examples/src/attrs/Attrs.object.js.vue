<script>
import { defineComponent } from "vue";

export default defineComponent({
  name: "Attrs",
  inheritAttrs: false,
  props: {
    label: {
      type: String,
      required: true,
    },
    disabled: {
      type: Boolean,
      default: false,
    },
  },
  emits: ["click", "focus"],
  computed: {
    attrClass() {
      return this.$attrs.class;
    },
    attrStyle() {
      return this.$attrs.style;
    },
    attrId() {
      return this.$attrs.id;
    },
    tabindex() {
      const value = this.$attrs.tabindex;
      return typeof value === "number"
        ? value
        : typeof value === "string"
        ? parseInt(value, 10)
        : 0;
    },
    placeholder() {
      return this.$attrs.placeholder;
    },
    title() {
      return this.$attrs.title;
    },
    role() {
      return this.$attrs.role;
    },
    dataAttrs() {
      const dataAttrs = {};
      for (const key in this.$attrs) {
        if (key.startsWith("data-")) {
          dataAttrs[key] = this.$attrs[key];
        }
      }
      return dataAttrs;
    },
    ariaAttrs() {
      const ariaAttrs = {};
      for (const key in this.$attrs) {
        if (key.startsWith("aria-")) {
          ariaAttrs[key] = this.$attrs[key];
        }
      }
      return ariaAttrs;
    },
    remainingAttrs() {
      const { class: _, style: __, id: ___, ...rest } = this.$attrs;
      return rest;
    },
  },
  methods: {
    handleClick(event) {
      this.$emit("click", event);
    },
    handleFocus(event) {
      this.$emit("focus", event);
    },
  },
});
</script>

<template>
  <div class="attrs-wrapper">
    <label :for="attrId">{{ label }}</label>
    <input
      v-bind="$attrs"
      :disabled="disabled"
      @click="handleClick"
      @focus="handleFocus"
    />
    
    <div class="custom-element">
      <span :class="attrClass" :style="attrStyle">
        Custom styled element
      </span>
    </div>
    
    <div class="attrs-info">
      <p>ID: {{ attrId }}</p>
      <p>Tabindex: {{ tabindex }}</p>
      <p>Placeholder: {{ placeholder }}</p>
      <p>Title: {{ title }}</p>
      <p>Role: {{ role }}</p>
      <p>Data attrs: {{ dataAttrs }}</p>
      <p>Aria attrs: {{ ariaAttrs }}</p>
    </div>
  </div>
</template>
