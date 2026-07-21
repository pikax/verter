<script setup lang="ts">
// Vue-FREE on purpose: the ONLY resolution that can fail here is the
// project-binding mechanism this fixture characterizes. The solution
// `tsconfig.json` (`files: [], references: […]`) references TWO leaves
// (`tsconfig.app.json` + `tsconfig.components.json`) that BOTH `include` `src`,
// so this carrier is claimed by MULTIPLE configured projects. Pre-fix that was a
// terminal `Ambiguous(MultipleOwners)` (no serve — TS2307/TS2304, no types);
// post-fix tsgo `GetDefaultProject` binds it to the FIRST leaf in the solution's
// declared references order (`tsconfig.app.json`) and it type-checks normally.
// No `import "vue"` / `defineProps`, so a missing `vue` dependency can never
// contaminate the assertion.

// Path-aliased import — resolves ONLY when the carrier is a member of a leaf
// configured project (its `paths` mapping applies).
import { makeShape, type AliasedShape } from "@/lib/helper";

const shape: AliasedShape = makeShape(1, "hello");

// Ambient global reached only via a leaf tsconfig `types` + `typeRoots`.
const token = AMBIENT_TOKEN;
</script>
<template>
  <div>{{ shape.label }} {{ token }}</div>
</template>
