<script setup lang="ts">
import { ref, provide } from 'vue'
import DeepMiddle from './DeepMiddle.vue'

const rootValue = ref('root-initial')

provide('deep-value', rootValue)
provide('update-root', () => {
  rootValue.value = 'leaf-updated'
})
</script>

<template>
  <div data-testid="deep-nested">
    <span data-testid="deep-root-value">{{ rootValue }}</span>
    <DeepMiddle />
  </div>
</template>
