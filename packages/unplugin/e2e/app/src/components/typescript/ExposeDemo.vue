<script setup lang="ts">
import { ref } from 'vue'

const internalCount = ref(0)

function increment() {
  internalCount.value++
}

function getCount() {
  return internalCount.value
}

defineExpose({ increment, getCount })
</script>

<template>
  <div data-testid="expose-demo">
    <span data-testid="expose-count">{{ internalCount }}</span>
  </div>
</template>
