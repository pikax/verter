<script setup lang="ts">
import { ref } from "vue";
// DEFAULT import of a barrel whose only export is a NAMED re-export.
// TS rejects this with TS1192 ("Module has no default export") before any
// prop check on `<Idx>`. The barrel is imported RELATIVELY so the rejection
// does not depend on tsconfig `paths` resolution (which diverges across
// providers in the carrier-diagnostics program).
import Idx from "./components";

const count = ref(0);
</script>
<template>
  <div>{{ count }}</div>
  <Idx caption="hello" />
</template>
