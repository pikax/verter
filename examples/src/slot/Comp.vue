<script setup lang="ts">
defineProps<{
  name: string;
}>();

defineSlots<{ header: (a: { foo: string }) => any }>();
</script>
<template>
  <div>
    <slot name="header" :foo="'1'"></slot>
  </div>
</template>
