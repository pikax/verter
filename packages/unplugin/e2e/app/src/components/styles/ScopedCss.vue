<script setup lang="ts">
import { ref } from 'vue'
const clicked = ref(false)
</script>

<template>
  <div data-testid="scoped-css">
    <p data-testid="scoped-text" class="scoped-red">Scoped Red Text</p>
    <button data-testid="scoped-btn" @click="clicked = !clicked">Toggle</button>
    <span data-testid="scoped-state">{{ clicked }}</span>
  </div>
</template>

<style scoped>
.scoped-red {
  color: rgb(255, 0, 0);
}
</style>
