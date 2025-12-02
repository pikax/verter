<script setup>
import { useAttrs, computed } from "vue";

// useAttrs in script setup
const attrs = useAttrs();

// Define props explicitly
const props = defineProps({
  label: {
    type: String,
    required: true,
  },
  disabled: {
    type: Boolean,
    default: false,
  },
});

const emit = defineEmits(["click", "focus"]);

function getAttrClass() {
  return attrs.class;
}

function getAttrStyle() {
  return attrs.style;
}

function getAttrId() {
  return attrs.id;
}

function getDataAttrs() {
  const dataAttrs = {};
  for (const key in attrs) {
    if (key.startsWith("data-")) {
      dataAttrs[key] = attrs[key];
    }
  }
  return dataAttrs;
}

function getAriaAttrs() {
  const ariaAttrs = {};
  for (const key in attrs) {
    if (key.startsWith("aria-")) {
      ariaAttrs[key] = attrs[key];
    }
  }
  return ariaAttrs;
}

function getRemainingAttrs() {
  const { class: _, style: __, id: ___, ...rest } = attrs;
  return rest;
}

function handleClick(event) {
  emit("click", event);
}

function handleFocus(event) {
  emit("focus", event);
}

const tabindex = computed(() => {
  const value = attrs.tabindex;
  return typeof value === "number" ? value : typeof value === "string" ? parseInt(value, 10) : 0;
});

const placeholder = computed(() => attrs.placeholder);
const title = computed(() => attrs.title);
const role = computed(() => attrs.role);
</script>

<script>
export default {
  inheritAttrs: false,
};
</script>

<template>
  <div class="attrs-wrapper">
    <label :for="getAttrId()">{{ label }}</label>
    <input
      v-bind="attrs"
      :disabled="disabled"
      @click="handleClick"
      @focus="handleFocus"
    />
    
    <div class="custom-element">
      <span :class="getAttrClass()" :style="getAttrStyle()">
        Custom styled element
      </span>
    </div>
    
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
