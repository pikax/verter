<script setup lang="ts">
import { ref } from "vue";
import IsoChild from "./IsoChild.vue";

// Under isolatedDeclarations, every exported binding must have an explicit,
// emit-inferable type. A `.vue` carrier's generated TSX must not produce
// spurious TS9xxx isolated-declaration errors for the import or the component.
const count: number = ref(0).value;
</script>
<template>
  <div>{{ count }}</div>
  <IsoChild isoChildOnly="a" />
</template>
