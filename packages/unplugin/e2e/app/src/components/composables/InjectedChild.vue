<script setup lang="ts">
import { inject } from 'vue'

const theme = inject<string>('theme', 'default-theme')
</script>

<template>
  <div data-testid="injected-child">
    <span data-testid="injected-value">{{ theme }}</span>
  </div>
</template>
