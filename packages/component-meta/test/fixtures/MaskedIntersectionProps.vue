<script setup lang="ts">
import type { Bad } from './masked-intersection-bad'

defineProps<{ x: string } & Bad>()
</script>

<template>
  <div />
</template>
