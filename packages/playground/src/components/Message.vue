<script setup lang="ts">
defineProps<{
  errors: string[]
}>()
</script>

<template>
  <div v-if="errors.length > 0" class="message-container">
    <div v-for="(error, i) in errors" :key="i" class="message error">
      {{ error }}
    </div>
  </div>
</template>

<style scoped>
.message-container {
  padding: 8px;
  background: var(--bg-secondary);
  border-top: 1px solid var(--border-color);
}

.message {
  padding: 8px 12px;
  border-radius: 4px;
  font-size: 13px;
  font-family: monospace;
}

.message.error {
  background: #ffebee;
  color: #c62828;
  border-left: 3px solid #c62828;
}

html.dark .message.error {
  background: #2a1a1a;
  color: #ff6b6b;
}
</style>
