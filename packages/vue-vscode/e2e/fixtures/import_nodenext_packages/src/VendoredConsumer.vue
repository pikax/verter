<script setup lang="ts">
import { ref } from "vue";
// Imports a RAW `.vue` SFC from node_modules (the package is materialized by
// the test at runtime). Resolving its props requires Verter to synthesize a
// carrier surface from the external node_modules `.vue` source.
import Vendored from "@pkg/vuecomp/Vendored.vue";

const count = ref(0);
</script>
<template>
  <div>{{ count }}</div>
  <Vendored vendoredVueOnly="a" />
</template>
