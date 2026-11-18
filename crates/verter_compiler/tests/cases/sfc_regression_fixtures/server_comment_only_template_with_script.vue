<script setup lang="ts">
/** Reads the value whenever the component is rendered. */
const props = defineProps<{ read: () => Promise<void> }>();
function run(task: () => Promise<void>) {
  return task;
}
run(() => props.read());
</script>

<template>
  <!-- shows `x` -->
</template>
