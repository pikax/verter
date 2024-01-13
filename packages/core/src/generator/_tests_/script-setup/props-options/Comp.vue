<script setup>
const props = defineProps({
  foo: String,
  bar: {
    type: String,
    default: "bar",
  },
});
</script>
<template>
  <span>1</span>
</template>
