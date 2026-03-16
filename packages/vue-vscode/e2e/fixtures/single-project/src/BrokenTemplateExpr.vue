<script setup lang="ts">
import { computed, ref } from 'vue'

const count = ref(1)
const formatted = computed(() => `count:${count.value}`)
</script>

<template>
  <p>{{ count }}</p>
  <p>{{ count + }}</p>
  <p>{{ formatted }}</p>
</template>
