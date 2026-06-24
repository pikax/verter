<script setup lang="ts">
// Path-aliased import — resolves ONLY when the carrier is a member of the
// configured project (the `paths` mapping applies). The inferred path emits a
// false TS2307 here.
import { makeShape, type AliasedShape } from "@/lib/helper";

const shape: AliasedShape = makeShape(1, "hello");

// Ambient global reached only via tsconfig `types` + `typeRoots`. The inferred
// path emits TS2304 (cannot find name) here.
const token = AMBIENT_TOKEN;
</script>

<template>
  <div>{{ shape.label }} {{ token }}</div>
</template>
