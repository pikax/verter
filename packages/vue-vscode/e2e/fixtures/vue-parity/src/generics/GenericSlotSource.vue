<script setup lang="ts" generic="T">
defineProps<{ value: T }>();
defineSlots<{ default(props: { value: T }): unknown }>();
</script>

<template><slot :value="value" /></template>
