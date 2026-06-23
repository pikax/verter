<script setup lang="ts">
import { ref } from "vue";
// Side-effect import: resolves the module, registers NO component binding.
import "./sideEffect";
// Dynamic import of a `.vue` carrier — provider passthrough; Verter does not
// discover this as a template component (tracked gap for async-component disco).
const Lazy = () => import("./DirectComp.vue");
// A deliberately broken import path — surfaces module-not-found (TS2307).
import { Missing } from "./does-not-exist";

const count = ref(0);
void Lazy;
void Missing;
</script>
<template>
  <div>{{ count }}</div>
</template>
