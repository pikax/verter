<script setup>
// @ts-check
import JsPropChild from "./JsPropChild.vue";
</script>

<template>
  <!-- Live type error under @ts-check / provider: count is number -->
  <JsPropChild title="ok" :count="'not-a-number'" />
</template>
