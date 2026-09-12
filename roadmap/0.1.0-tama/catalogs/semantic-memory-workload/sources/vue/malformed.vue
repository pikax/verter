<script setup lang="ts">
// Fixed workload fixture for the FAILED-CONSTRUCTION action class. The
// script block is deliberately unterminated: construction of this file's
// semantic artifact must fail, must release its reservations, and must
// never warm a cache entry. Frozen bytes — do not repair this file.
import type { PropsBase } from "../shared/props-base";

const props = defineProps<PropsBase<
</script>

<template>
  <div>{{ props }}</div>
