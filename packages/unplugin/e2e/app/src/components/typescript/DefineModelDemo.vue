<script setup lang="ts">
const modelValue = defineModel<string>({ default: '' })
</script>

<template>
  <div data-testid="define-model-demo">
    <input data-testid="model-input" v-model="modelValue" />
    <span data-testid="model-display">{{ modelValue }}</span>
  </div>
</template>
