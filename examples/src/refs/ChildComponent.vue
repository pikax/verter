<script setup lang="ts">
// Child component with exposed methods

import { ref, defineOptions } from "vue";

const internalValue = ref("");
const inputRef = ref<HTMLInputElement | null>(null);

defineOptions({
  inheritAttrs: false,
});

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

const a = {} as ___VERTER___RootElement;
const b = {} as ___VERTER___Instance;
b.$el;
// ___VERTER___defineOptions_Boxed = ___VERTER___defineOptions_Box({
//   inheritAttrs: false
// })

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
