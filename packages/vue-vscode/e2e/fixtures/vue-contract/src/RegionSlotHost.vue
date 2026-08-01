<script setup lang="ts">
// A child with a typed public surface in every dimension a completion source has to
// read: declared props, declared emits, and BOTH a default and a named scoped slot.
// Without a typed surface a completion assertion cannot tell "offered the right
// things" from "offered whatever the HTML fallback had".
defineProps<{ hostProp?: string }>();

defineEmits<{
  hostPing: [value: string];
}>();

defineSlots<{
  default(props: { hostDatum: string }): unknown;
  hostFooter(props: { footerDatum: number }): unknown;
}>();

const hostSource = "slotted";
</script>

<template>
  <slot :hostDatum="hostSource" />
  <slot name="hostFooter" :footerDatum="1" />
</template>
