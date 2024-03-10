<script setup lang="ts">
const slots = defineSlots<{ foo: () => any }>();
</script>
<template>
  <div>
    <slot name="foo" />

    <!-- @ts-expect-error  -->
    <slot name="bar" />
  </div>
</template>
