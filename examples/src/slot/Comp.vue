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

const vFoo = (
  el: HTMLInputElement,
  binding: { value: string; oldValue: string }
) => {
  el.value = binding.value + binding.oldValue;
};
</script>
<template>
  <div v-foo="'test'">
    <slot name="header" :foo="'1'"></slot>
  </div>
</template>
