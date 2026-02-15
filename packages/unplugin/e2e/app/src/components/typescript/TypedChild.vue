<script setup lang="ts">
defineProps<{
  message: string
}>()

const emit = defineEmits<{
  reply: [value: string]
}>()
</script>

<template>
  <div data-testid="typed-child">
    <span data-testid="child-message">{{ message }}</span>
    <button data-testid="child-reply" @click="emit('reply', 'child-reply')">Reply</button>
  </div>
</template>
