<script lang="ts" setup>
import { withDefaults, defineProps } from 'vue';

const props = withDefaults(
  defineProps({
    bar: String,
  }),
  {},
);

const d = defineProps<{
  a: string
}>()


</script>

<template>
  <div>{{ props }}</div>
  <div>{{ $props }}</div>
  <div>{{ bar }}</div>
</template>
