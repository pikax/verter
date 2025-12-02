<script setup>
import { ref } from "vue";

const inputRef = ref(null);

function focus() {
  inputRef.value?.focus();
}

function getValue() {
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
