<script setup lang="ts">
const props = defineProps<{
  msg: string;
}>();

const emit = defineEmits<{
  reset: [];
}>();

defineSlots<{
  default?: (props: { msg: string }) => unknown;
}>();

function reset(): void {
  emit("reset");
  void props.msg;
}

defineExpose({ reset });
</script>

<template>
  <p>{{ msg }}</p>
  <slot :msg="msg" />
</template>
