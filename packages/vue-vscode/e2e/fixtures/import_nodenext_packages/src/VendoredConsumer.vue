<script setup lang="ts">
import { ref } from "vue";
// Imports a RAW `.vue` SFC from node_modules (the package is materialized by
// the test at runtime). Resolving its props requires generating a carrier from
// external node_modules `.vue` source — a hard-STOP boundary.
import Vendored from "@pkg/vuecomp/Vendored.vue";

const count = ref(0);
</script>
<template>
  <div>{{ count }}</div>
  <Vendored vendoredVueOnly="a" />
</template>
