<script setup lang="ts">
// Slot typing errors as script-level type mismatches
interface SlotProps {
  active: boolean
  count: number
}

// TS2339 — boolean has no method toFixed
function processSlot(props: SlotProps) {
  const x = props.active.toFixed(2)
  const y = props.count.toLowerCase()
  return x + y
}
</script>
<template>
  <div>{{ processSlot({ active: true, count: 0 }) }}</div>
</template>
