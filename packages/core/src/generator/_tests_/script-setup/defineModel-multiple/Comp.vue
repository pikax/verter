<script setup lang="ts">
const testModel = defineModel<{ foo: string }>("test");
const model = defineModel({
  default: false,
});
</script>
<template>
  <span>1</span>
</template>
