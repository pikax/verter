<script setup lang="ts">
import { ref } from "vue";
const themeColor = ref("rgb(255, 0, 0)");

function changeColor() {
  themeColor.value = themeColor.value === "rgb(255, 0, 0)" ? "rgb(0, 0, 255)" : "rgb(255, 0, 0)";
}
</script>

<template>
  <div data-testid="css-v-bind">
    <p data-testid="vbind-text" class="dynamic-color">Dynamic Color</p>
    <button data-testid="vbind-change" @click="changeColor">Change Color</button>
  </div>
</template>

<style scoped>
.dynamic-color {
  color: v-bind(themeColor);
}
</style>
