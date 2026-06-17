<template>
  <div v-bind="$attrs" />
</template>

<script setup lang="ts">
import { useAttrs } from "vue";

interface FallthroughAttrs {
  id?: string;
  onClick?: (event: MouseEvent) => void;
}

const attrs = useAttrs() as FallthroughAttrs;

const rootId = attrs.id; // @dx-anchor attrs.id
const rootClick = attrs.onClick; // @dx-anchor attrs.onClick
</script>
