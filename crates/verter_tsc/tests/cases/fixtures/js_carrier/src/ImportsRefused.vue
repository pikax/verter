<script setup lang="ts">
// A consumer of the REFUSED SFC. `MixedLang.vue` generates no companion at all,
// so this import falls through to the ambient `declare module '*.vue'` shim
// (`DefineComponent<{}, {}, any>`) instead of failing to resolve.
//
// That is the pinned importer experience: refusing one file does NOT cascade a
// TS2307 through every module that mentions it. The contrast is
// `TsParentArity.vue`, whose import of an ADMITTED SFC gets the real generated
// surface — where a wrong call IS caught.
import Mixed from './MixedLang.vue'

const shown = Mixed
</script>
<template>
  <Mixed />
  <div>{{ typeof shown }}</div>
</template>
