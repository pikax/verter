<script setup lang="ts">
// Imports a CLEAN symbol from a NON-root module that ALSO contains a real type
// error (nonRootBad.ts). This SFC is itself clean — its only role is to pull
// nonRootBad.ts into the program so the whole-program typecheck reaches the
// non-root error. Under the old per-root loop that error was never queried.
import { answer } from "./nonRootBad";

const doubled: number = answer * 2;
</script>
<template>
  <div>{{ doubled }}</div>
</template>
