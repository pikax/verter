<script setup lang="ts" generic="T, U">
const props = defineProps<{
  rows?: T[];
  project?: (row: T) => U;
  modelValue?: U;
}>();

const emit = defineEmits<{
  change: [value: U];
  "update:modelValue": [value: U];
}>();

defineSlots<{
  default?: (props: { row: T; value: U }) => unknown;
}>();

defineExpose({
  value: undefined as unknown as U,
});

void props;
void emit;
</script>

<template>
  <slot :row="(rows && rows[0]) as T" :value="undefined as unknown as U" />
</template>
