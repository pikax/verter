<script setup lang="ts">
import VueChild from "./VueChild.vue";
import SvelteChild from "./SvelteChild.svelte";
</script>

<template>
  <!-- Cross-framework wrong props must still type-error in a mixed project -->
  <VueChild :title="42" />
  <SvelteChild :label="99" />
</template>
