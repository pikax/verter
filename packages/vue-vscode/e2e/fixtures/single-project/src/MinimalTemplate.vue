<script setup lang="ts">
import { computed, ref } from "vue";

const base = ref(1);
const doubled = computed(() => base.value * 2);
</script>

<template>
  <div>{{ doubled }}</div>
</template>
