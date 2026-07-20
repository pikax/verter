<script setup lang="ts">
import type { Item } from "../../support/types"

const props = withDefaults(
  defineProps<{
    item: Item
    size?: "sm" | "lg"
    tags?: string[]
  }>(),
  { size: "sm", tags: () => [] },
)
const emit = defineEmits<{
  pick: [item: Item]
  close: []
}>()
</script>

<template>
  <div @click="emit('pick', props.item)">
    {{ item.name }} ({{ size }}) {{ tags.length }}
  </div>
</template>
