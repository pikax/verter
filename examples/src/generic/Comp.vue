<script setup lang="ts" generic="T">
defineProps<{
  name: string;
}>();

// const slots = defineSlots<{
//   foo: (a: { name: T }) => any;
// }>();

defineSlots<
  Record<T & string, () => any> & { header: (a: { foo: string }) => any }
>();
</script>
<template>
  <div>
    <slot name="header"></slot>
  </div>
</template>
