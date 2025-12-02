<script setup lang="ts">
import { useAttrs, computed, type SetupContext } from "vue";

// useAttrs in script setup
const attrs = useAttrs();

// Define props explicitly - these won't be in attrs
const props = defineProps<{
  label: string;
  disabled?: boolean;
}>();

// Define emits
const emit = defineEmits<{
  click: [event: MouseEvent];
  focus: [event: FocusEvent];
}>();

// Using attrs
function getAttrClass(): string | undefined {
  return attrs.class as string | undefined;
}

function getAttrStyle(): string | Record<string, string> | undefined {
  return attrs.style as string | Record<string, string> | undefined;
}

function getAttrId(): string | undefined {
  return attrs.id as string | undefined;
}

function getDataAttrs(): Record<string, unknown> {
  const dataAttrs: Record<string, unknown> = {};
  for (const key in attrs) {
    if (key.startsWith("data-")) {
      dataAttrs[key] = attrs[key];
    }
  }
  return dataAttrs;
}

function getAriaAttrs(): Record<string, unknown> {
  const ariaAttrs: Record<string, unknown> = {};
  for (const key in attrs) {
    if (key.startsWith("aria-")) {
      ariaAttrs[key] = attrs[key];
    }
  }
  return ariaAttrs;
}

// Spread remaining attrs
function getRemainingAttrs(): Record<string, unknown> {
  const { class: _, style: __, id: ___, ...rest } = attrs;
  return rest;
}

// Event handlers
function handleClick(event: MouseEvent) {
  emit("click", event);
}

function handleFocus(event: FocusEvent) {
  emit("focus", event);
}

// Access specific attrs with typing
const tabindex = computed(() => {
  const value = attrs.tabindex;
  return typeof value === "number" ? value : typeof value === "string" ? parseInt(value, 10) : 0;
});

const placeholder = computed(() => attrs.placeholder as string | undefined);
const title = computed(() => attrs.title as string | undefined);
const role = computed(() => attrs.role as string | undefined);
</script>

<script lang="ts">
// inheritAttrs: false prevents automatic attribute inheritance
export default {
  inheritAttrs: false,
};
</script>

<template>
  <div class="attrs-wrapper">
    <!-- Manually apply attrs to inner element -->
    <label :for="getAttrId()">{{ label }}</label>
    <input
      v-bind="attrs"
      :disabled="disabled"
      @click="handleClick"
      @focus="handleFocus"
    />
    
    <!-- Or selectively apply attrs -->
    <div class="custom-element">
      <span :class="getAttrClass()" :style="getAttrStyle()">
        Custom styled element
      </span>
    </div>
    
    <!-- Show attrs info -->
    <div class="attrs-info">
      <p>ID: {{ getAttrId() }}</p>
      <p>Tabindex: {{ tabindex }}</p>
      <p>Placeholder: {{ placeholder }}</p>
      <p>Title: {{ title }}</p>
      <p>Role: {{ role }}</p>
      <p>Data attrs: {{ getDataAttrs() }}</p>
      <p>Aria attrs: {{ getAriaAttrs() }}</p>
    </div>
  </div>
</template>
