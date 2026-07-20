<template>
  <ul>
    <li v-for="item in items" :key="item.id" @select="pick(item)">
      <span v-if="item.ready">{{ item.name }}</span>
    </li>
  </ul>
</template>

<script setup lang="ts">
import { formatLabel } from "../../lib/format";

const props = defineProps<{ items: { id: string; name: string; ready: boolean }[] }>();
const emit = defineEmits<{ select: [id: string] }>();

function pick(item: { id: string }): void {
  emit("select", formatLabel(item.id));
}
</script>
