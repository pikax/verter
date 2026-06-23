<script setup lang="ts">
import { ref } from "vue";
// Side-effect import: resolves the module, registers NO component binding.
import "./sideEffect";
// A bare dynamic-import arrow (`() => import(...)`) is a plain value, not a
// component declaration. It is rendered as a `<Lazy>` tag below so the negative
// "not a template component" classification is forceable: a binding wrongly
// classified as a component would appear in the template-component analysis.
const Lazy = () => import("./DirectComp.vue");
// A deliberately broken import path — surfaces module-not-found (TS2307).
import { Missing } from "./does-not-exist";

const count = ref(0);
void Missing;
</script>
<template>
  <div>{{ count }}</div>
  <Lazy directOnly="a" />
</template>
