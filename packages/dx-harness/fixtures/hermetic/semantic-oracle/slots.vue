<template>
  <div>
    <slot name="header" :title="heading" />
    <slot :item="row" />
  </div>
</template>

<script setup lang="ts">
const heading = "Header";
const row = "row-1";

const slots = defineSlots<{
  default(props: { item: string }): unknown;
  header(props: { title: string }): unknown;
}>();

const renderDefault = slots.default; // @dx-anchor slots.default
const renderHeader = slots.header; // @dx-anchor slots.header
</script>
