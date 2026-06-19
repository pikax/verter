<script setup lang="ts">
import type { Status } from "./types";

defineProps<{
  status: Status;
  label?: string;
}>();

const colorMap: Record<Status, string> = {
  active: "green",
  inactive: "gray",
  pending: "yellow",
};
</script>
<template>
  <span :class="colorMap[status]">
    {{ label ?? status }}
  </span>
</template>
