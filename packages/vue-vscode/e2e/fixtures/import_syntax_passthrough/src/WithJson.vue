<script setup lang="ts">
import { ref } from "vue";
// JSON import attribute (`with { type: "json" }`) — TS 5.3+/6/7 accept this.
// Verter must PRESERVE the attribute through IDE-TSX codegen so the provider
// still accepts it (no spurious error) and `data.jsonValueOnly` types.
import data from "./data.json" with { type: "json" };

const count = ref(0);
const value = data.jsonValueOnly;
</script>
<template>
  <div>{{ count }} {{ value }}</div>
</template>
