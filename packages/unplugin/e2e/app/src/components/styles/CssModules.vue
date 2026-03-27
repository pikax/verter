<script setup lang="ts">
import { ref } from "vue";
const toggled = ref(false);
</script>

<template>
  <div data-testid="css-modules">
    <p data-testid="module-text" :class="toggled ? $style.active : $style.inactive">Module Text</p>
    <button data-testid="module-toggle" @click="toggled = !toggled">Toggle Module</button>
  </div>
</template>

<style module>
.active {
  color: rgb(0, 128, 0);
  font-weight: bold;
}
.inactive {
  color: rgb(128, 128, 128);
}
</style>
