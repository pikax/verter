<script setup lang="ts">
import { ref } from "vue";
// Deferred namespace import (`import defer * as ns`, TS 5.9+). Legal under
// `module: preserve`. Verter must PRESERVE the `defer` modifier through IDE-TSX
// codegen so the namespace binding still resolves and emits no spurious error.
import defer * as deferred from "./deferred";

const count = ref(0);
const value = deferred.deferredValueOnly;
</script>
<template>
  <div>{{ count }} {{ value }}</div>
</template>
