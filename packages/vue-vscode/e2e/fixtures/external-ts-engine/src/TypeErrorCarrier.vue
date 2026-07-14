<script setup lang="ts">
// A DELIBERATE semantic type error in a carrier `<script setup>`: a string
// literal assigned to a `number` binding. Under the project-bound external-TS
// engine this MUST surface TS2322 ("Type 'string' is not assignable to type
// 'number'") on the carrier's `.vue` source — proving carrier diagnostics
// positively flow (a PRESENCE check, unlike the absence-only baselines).
const typedNumber: number = "definitely not a number";
</script>

<template>
  <div>{{ typedNumber }}</div>
</template>
