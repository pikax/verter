<script lang="js" setup>
// JavaScript setup script
const count = 0;
const name = "John";

defineProps({
  title: String,
  count: Number,
});

defineEmits(["change", "update"]);
</script>

<template>
  <div>
    <p>{{ name }}: {{ count }}</p>
  </div>
</template>
