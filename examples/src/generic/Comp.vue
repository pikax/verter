<script setup lang="ts" generic="T extends string">
defineProps<{
  name: T;
}>();

defineEmits<{
  change: [name: T];
}>();

// const slots = defineSlots<{
//   foo: (a: { name: T }) => any;
// }>();

defineSlots<
  Record<T & string, (args: { test: T }) => any> & {
    header: (a: { foo: string }) => any;
  }
>();

async () => {
  await Promise.resolve();
};

// const a = {} as ReturnType<typeof ___VERTER___getRootComponent<T>> 
</script>
<template>
  <div>
    <slot name="header" foo=""> {{ name }}</slot>
    <slot :name="name" :testx="'aaa'"></slot>
    <slot name="testx" :te="aaa"></slot>
  </div>
</template>
