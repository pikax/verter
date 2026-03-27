<script setup lang="ts">
import { ref } from "vue";

const isActive = ref(false);
const hasError = ref(false);
const color = ref("blue");

function toggleActive() {
  isActive.value = !isActive.value;
}
function toggleError() {
  hasError.value = !hasError.value;
}
function toggleColor() {
  color.value = color.value === "blue" ? "red" : "blue";
}
</script>

<template>
  <div data-testid="dynamic-bindings">
    <div data-testid="class-target" :class="{ active: isActive, error: hasError }">
      Class Target
    </div>
    <button data-testid="toggle-active" @click="toggleActive">Toggle Active</button>
    <button data-testid="toggle-error" @click="toggleError">Toggle Error</button>

    <div data-testid="style-target" :style="{ color: color, fontWeight: 'bold' }">Style Target</div>
    <button data-testid="toggle-color" @click="toggleColor">Toggle Color</button>
  </div>
</template>
