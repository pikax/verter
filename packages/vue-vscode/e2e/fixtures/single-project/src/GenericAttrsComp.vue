<script setup lang="ts" generic="T extends string" attrs="{ class: string, id?: string }">
import { ref } from "vue";

defineProps<{ value: T }>();
const typedValue = ref<T>();
</script>
<template>
  <div>{{ value }} {{ typedValue }}</div>
</template>
