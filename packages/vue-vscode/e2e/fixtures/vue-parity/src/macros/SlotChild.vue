<script setup lang="ts">
defineSlots<{
  default(props: { slotItem: { name: string }; slotIndex: number }): any;
}>();
</script>

<template>
  <div>
    <slot :slot-item="{ name: 'a' }" :slot-index="0" />
  </div>
</template>
