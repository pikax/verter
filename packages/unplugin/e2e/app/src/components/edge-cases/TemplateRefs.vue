<script setup lang="ts">
import { ref } from "vue";

const inputRef = ref<HTMLInputElement | null>(null);
const readValue = ref("");

function focusInput() {
  inputRef.value?.focus();
}

function readInputValue() {
  readValue.value = inputRef.value?.value ?? "";
}
</script>

<template>
  <div data-testid="template-refs">
    <input ref="inputRef" data-testid="ref-input" value="hello-ref" />
    <button data-testid="ref-focus" @click="focusInput">Focus</button>
    <button data-testid="ref-read" @click="readInputValue">Read Value</button>
    <span data-testid="ref-value">{{ readValue }}</span>
  </div>
</template>
