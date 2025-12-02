<script setup lang="ts">
import { ref } from "vue";

const inputRef = ref<HTMLInputElement | null>(null);

function focus() {
  inputRef.value?.focus();
}

function getValue(): string {
  return inputRef.value?.value ?? "";
}

defineExpose({
  focus,
  getValue,
});
</script>

<template>
  <div>
    <input ref="inputRef" type="text" />
    <button @click="focus">Focus</button>
  </div>
</template>
