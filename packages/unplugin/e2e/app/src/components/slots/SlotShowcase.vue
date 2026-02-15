<script setup lang="ts">
import SlotWrapper from './SlotWrapper.vue'
</script>

<template>
  <div data-testid="slot-showcase">
    <SlotWrapper>
      <template #header>
        <span data-testid="custom-header">Custom Header Content</span>
      </template>

      <template #default>
        <span data-testid="custom-default">Custom Default Content</span>
      </template>

      <template #footer>
        <span data-testid="custom-footer">Custom Footer Content</span>
      </template>

      <template #scoped="{ count, increment }">
        <span data-testid="scoped-count">{{ count }}</span>
        <button data-testid="scoped-increment" @click="increment">Scoped Increment</button>
      </template>
    </SlotWrapper>
  </div>
</template>
