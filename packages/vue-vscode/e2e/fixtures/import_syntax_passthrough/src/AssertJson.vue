<script setup lang="ts">
import { ref } from "vue";
// Deprecated import assertion (`assert { type: "json" }`). TS 6/7 surface
// TS2880 ("Import assertions have been replaced by import attributes"). Verter
// must PRESERVE the provider's diagnostic mapped onto the carrier rather than
// corrupting or dropping it.
import data from "./data.json" assert { type: "json" };

const count = ref(0);
const value = data.jsonValueOnly;
</script>
<template>
  <div>{{ count }} {{ value }}</div>
</template>
