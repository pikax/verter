<script setup lang="ts" generic="T extends string">
defineProps<{
  name: T;
}>();

// const slots = defineSlots<{
//   foo: (a: { name: T }) => any;
// }>();

defineSlots<
  Record<T & string, (args: { name: T }) => any> & {
    header: (a: { foo: string }) => any;
  }
>();
</script>
<template>
  <div>
    <slot name="header" foo=""> {{ name }}</slot>
  </div>
</template>
