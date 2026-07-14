<script setup lang="ts">
// A REAL Vue SFC whose props come from an IMPORTED type via `defineProps<T>()`,
// with a deliberate in-script mis-assignment: `label` is `string`, assigned to a
// `number` ⇒ TS2322. The imported prop type flows through the macro into the
// generated IDE carrier via the shared resolver; the error lands in a MAPPED script
// region and must map back onto this `.vue` source (never a forged (0,0)).
//
// The props are reactive-destructured so `label`/`count` are bare script bindings —
// their interpolations (`{{ label }}`) are the cross-fixture-supported carrier hover
// surface (the Carrier IDE TS Surface Principle covers `{{ }}` interpolations), the
// same surface the non-gated `hover on prop binding in template` case exercises.
import type { LabelProps } from "./props";

const { label, count } = defineProps<LabelProps>();

// DELIBERATE TS2322: string is not assignable to number.
const wrong: number = label;
</script>

<template>
  <div>{{ label }} / {{ count }} / {{ wrong }}</div>
</template>
