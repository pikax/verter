<script setup lang="ts">
import TypeNegChild from "./TypeNegChild.vue";
</script>

<template>
  <!-- Live type errors (no @ts-expect-error): string where number is required -->
  <TypeNegChild title="ok" :count="'not-a-number'" />
  <!-- number where string is required -->
  <TypeNegChild :title="99" :count="1" />
  <!-- string where boolean is required -->
  <TypeNegChild title="ok" :count="1" :enabled="'yes'" />
</template>
