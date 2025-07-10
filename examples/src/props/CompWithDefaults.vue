<script lang="ts" setup>
import type { MyProps } from "./types";

const props = withDefaults(defineProps<MyProps>(), {});
</script>

<template>
  <div>{{ $props }}</div>
  <div>{{ bar }}</div>

  <span> {{ ___DEBUG_Verter }} </span>
  <span> {{ ___DEBUG_Default }} </span>
  <span> {{ ___DEBUG_Props }} </span>
  <span> {{ ___DEBUG_Components }} </span>
  <span> {{ ___DEBUG_FullContext }} </span>
  <span> {{ ___DEBUG_Binding }} </span>
</template>
