<script setup lang="ts">
defineProps<{
  name: string;
}>();
// defineProps<
//   | {
//       name: string;
//     }
//   | { foO: number }
//   | undefined
// >();
// defineProps<string>()
// defineProps<
//   [
//     {
//       name: string;
//     }
//   ]
// >();
// defineProps<
//   Array<{
//     name: string;
//   }>
// >();
// defineProps<true | { name: string }>();

defineSlots<{ header: (a: { foo: string }) => any }>();
</script>
<template>
  <div>
    <slot name="header" :foo="'1'"></slot>
  </div>
</template>
