<script setup lang="ts">
const foo = "foo";
</script>
<template>
  <div>
    <slot name="header" v-bind="{ title: 'Title-slot' }" />
    <slot />
    <slot name="footer" :foo="foo" />
  </div>
</template>
