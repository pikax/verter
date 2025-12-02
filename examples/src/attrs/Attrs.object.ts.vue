<script lang="ts">
import { defineComponent, type PropType } from "vue";

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
  emits: {
    click: (event: MouseEvent) => true,
    focus: (event: FocusEvent) => true,
  },
  computed: {
    attrClass(): string | undefined {
      return this.$attrs.class as string | undefined;
    },
    attrStyle(): string | Record<string, string> | undefined {
      return this.$attrs.style as string | Record<string, string> | undefined;
    },
    attrId(): string | undefined {
      return this.$attrs.id as string | undefined;
    },
    tabindex(): number {
      const value = this.$attrs.tabindex;
      return typeof value === "number"
        ? value
        : typeof value === "string"
        ? parseInt(value, 10)
        : 0;
    },
    placeholder(): string | undefined {
      return this.$attrs.placeholder as string | undefined;
    },
    title(): string | undefined {
      return this.$attrs.title as string | undefined;
    },
    role(): string | undefined {
      return this.$attrs.role as string | undefined;
    },
    dataAttrs(): Record<string, unknown> {
      const dataAttrs: Record<string, unknown> = {};
      for (const key in this.$attrs) {
        if (key.startsWith("data-")) {
          dataAttrs[key] = this.$attrs[key];
        }
      }
      return dataAttrs;
    },
    ariaAttrs(): Record<string, unknown> {
      const ariaAttrs: Record<string, unknown> = {};
      for (const key in this.$attrs) {
        if (key.startsWith("aria-")) {
          ariaAttrs[key] = this.$attrs[key];
        }
      }
      return ariaAttrs;
    },
    remainingAttrs(): Record<string, unknown> {
      const { class: _, style: __, id: ___, ...rest } = this.$attrs;
      return rest;
    },
  },
  methods: {
    handleClick(event: MouseEvent): void {
      this.$emit("click", event);
    },
    handleFocus(event: FocusEvent): void {
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
