<script setup lang="ts">
import VueChild from "./VueChild.vue";
import SvelteChild from "./SvelteChild.svelte";

const mixedLabel = "mixed";
</script>

<template>
  <main>
    <p>{{ mixedLabel }}</p>
    <VueChild :title="mixedLabel" />
    <SvelteChild :label="mixedLabel" />
  </main>
</template>
