<script setup lang="ts">
// A component exposing a NAMED model (`v-model:show`) via defineModel so a
// `v-model:show="..."` usage resolves the `show` prop type through the mapped
// prop-name codegen.
const show = defineModel<boolean>("show", { required: true });
</script>
<template>
  <div v-if="show">named model</div>
</template>
