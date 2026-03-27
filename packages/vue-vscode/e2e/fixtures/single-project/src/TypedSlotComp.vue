<script setup lang="ts">
interface SlotItem {
  id: number;
  name: string;
}

defineSlots<{
  default(props: { slotItem: SlotItem; slotIndex: number; slotTotal: number }): any;
}>();

const items: SlotItem[] = [{ id: 1, name: "alpha" }];
</script>

<template>
  <slot :slotItem="items[0]" :slotIndex="0" :slotTotal="items.length" />
</template>
