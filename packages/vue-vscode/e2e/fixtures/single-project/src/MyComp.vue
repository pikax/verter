<script setup lang="ts">
defineProps<{ foo: string; bar: number }>();
const emit = defineEmits<{ custom: [payload: string] }>();
</script>
<template>
  <div>
    {{ foo }} {{ bar }}
    <slot name="header" />
    <slot :item="foo" />
  </div>
</template>
