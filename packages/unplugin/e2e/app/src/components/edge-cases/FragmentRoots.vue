<script setup lang="ts">
import { ref } from 'vue'
const countA = ref(0)
const countB = ref(0)
</script>

<template>
  <div data-testid="fragment-root-a">
    <span data-testid="fragment-count-a">{{ countA }}</span>
    <button data-testid="fragment-btn-a" @click="countA++">A++</button>
  </div>
  <div data-testid="fragment-root-b">
    <span data-testid="fragment-count-b">{{ countB }}</span>
    <button data-testid="fragment-btn-b" @click="countB++">B++</button>
  </div>
</template>
