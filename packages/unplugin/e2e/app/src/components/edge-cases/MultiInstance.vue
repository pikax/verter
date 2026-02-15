<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
</script>

<template>
  <div data-testid="multi-instance">
    <span data-testid="instance-count">{{ count }}</span>
    <button data-testid="instance-btn" @click="count++">+</button>
  </div>
</template>
