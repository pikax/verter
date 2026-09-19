<script setup lang="ts" generic="T = unknown">
const props = defineProps<{
  test: T;
}>();

const emit = defineEmits<{
  change: [value: T];
}>();

defineSlots<{
  default?: (props: { value: T }) => unknown;
}>();

defineExpose({
  value: props.test,
  label: "generic",
});

void emit;
</script>

<template>
  <span><slot :value="test" /></span>
</template>
