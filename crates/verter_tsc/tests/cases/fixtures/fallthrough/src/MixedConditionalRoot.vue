<script setup lang="ts">
import ChildOptionalColliding from './ChildOptionalColliding.vue'
defineProps<{ pick: boolean }>()
</script>
<template>
  <ChildOptionalColliding v-if="pick" />
  <div v-else>fallback</div>
</template>
