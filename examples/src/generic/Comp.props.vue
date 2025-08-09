<script setup lang="ts" generic="T extends string">
import type { PropType } from "vue";

defineProps({
  name: String as unknown as PropType<T>,

  onName: Function as PropType<(n: T) => void>,
});

const emit = defineEmits<{
  close: [
    {
      name: T;
    }
  ];
}>();

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
