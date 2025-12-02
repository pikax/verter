<script setup lang="ts">
// Child component with exposed methods

import { ref } from "vue";

const internalValue = ref("");
const inputRef = ref<HTMLInputElement | null>(null);

function focus() {
  inputRef.value?.focus();
}

function reset() {
  internalValue.value = "";
}

function getValue(): string {
  return internalValue.value;
}

function someMethod() {
  console.log("someMethod called");
}

defineExpose({
  focus,
  reset,
  getValue,
  someMethod,
});
</script>

<template>
  <div>
    <input ref="inputRef" v-model="internalValue" />
  </div>
</template>
