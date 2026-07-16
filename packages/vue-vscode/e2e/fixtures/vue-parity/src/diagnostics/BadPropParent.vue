<script setup lang="ts">
import BadPropChild from "./BadPropChild.vue";
</script>

<template>
  <!-- title ok; count intentionally wrong type -->
  <BadPropChild title="x" :count="'not-a-number'" />
</template>
