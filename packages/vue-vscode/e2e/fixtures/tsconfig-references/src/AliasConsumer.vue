<script setup lang="ts">
// Vue-FREE on purpose (mirrors external-ts-engine/AliasConsumer.vue): the ONLY
// resolution that can fail here is the project-binding mechanism this fixture
// characterizes — a solution-style ROOT (`files: [], references: […]`) whose
// LEAF (`tsconfig.app.json`, NOT named `tsconfig.json`) owns the carrier and its
// `paths` / `types` / `typeRoots`. No `import "vue"` / `defineProps`, so a missing
// `vue` dependency can never contaminate the assertion.

// Path-aliased import — resolves ONLY when the carrier is a member of the LEAF
// configured project (its `paths` mapping applies).
import { makeShape, type AliasedShape } from "@/lib/helper";

const shape: AliasedShape = makeShape(1, "hello");

// Ambient global reached only via the LEAF tsconfig `types` + `typeRoots`.
const token = AMBIENT_TOKEN;
</script>
<template>
  <div>{{ shape.label }} {{ token }}</div>
</template>
