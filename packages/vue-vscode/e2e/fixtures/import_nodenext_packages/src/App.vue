<script setup lang="ts">
import { ref } from "vue";
// Package `exports` "." entry (vendored @pkg/ui, materialized by the test at
// runtime since `node_modules` is gitignored).
import { PkgComp } from "@pkg/ui";
// Package `imports` map (`#internal/*` -> `./src/internal/*`).
import { InternalComp } from "#internal/InternalComp.js";

const count = ref(0);
</script>
<template>
  <div>{{ count }}</div>
  <PkgComp pkgRootOnly="a" />
  <InternalComp internalOnly="b" />
</template>
