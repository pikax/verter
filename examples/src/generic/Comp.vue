<script setup lang="ts" generic="T extends string">
defineProps<{
  name: T;

  onName?: (n: T) => void;
}>();

// const slots = defineSlots<{
//   foo: (a: { name: T }) => any;
// }>();

defineSlots<
  Record<T & string, (args: { name: T }) => any> & {
    header: (a: { foo: string }) => any;
  }
>();

async () => {
  await Promise.resolve();
};
</script>
<template>
  <div>
    <slot name="header" foo=""> {{ name }}</slot>
  </div>
</template>
