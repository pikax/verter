<script setup lang="ts">
import Skipped from "../src/excluded/Skipped.svelte";
const n: number = "vue-only-must-still-check";
void Skipped;
</script>

<template>
  <p>{{ n }}</p>
</template>
