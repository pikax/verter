<script setup lang="ts">
// Script setup without lang attribute but with TypeScript content
// Tests parser handling of type inference
</script>

<script>
// Regular script without lang - should be treated as JS
export default {
  name: "NoLangComponent",
};
</script>

<template>
  <div>No lang attribute</div>
</template>
