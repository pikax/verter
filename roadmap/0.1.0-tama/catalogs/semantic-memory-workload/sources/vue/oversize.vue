<script setup lang="ts">
// Fixed workload TEMPLATE for the OVERSIZED-ENTRY boundary class: the
// resource contract requires "one entry exceeding the cache cap", and no
// hand-written component reaches that size. This template is therefore
// EXPANDED at materialization time rather than copied verbatim.
//
// Expansion rule (declared in catalogs/semantic-memory-budget.toml under
// [workload_materialization]): the region between the REPEAT-BEGIN and
// REPEAT-END markers below is emitted `oversize_repeat` times, with the
// ordinal placeholder `__N__` replaced by the zero-based repetition
// ordinal. The committed bytes hold exactly one un-expanded copy, so the
// reviewable surface stays one member rather than tens of thousands.
//
// Because the placeholder is not valid TypeScript, this template is never
// itself requested — only its expansion is. Frozen bytes; SHA-256 pinned
// by the budget catalog.
import type { Density, Labelled } from "../shared/props-base";

interface OversizeProps {
  rows: readonly Labelled[];
  density: Density;
  // REPEAT-BEGIN
  field__N__?: { id: string; label: string; density: Density } | null;
  // REPEAT-END
}

const props = defineProps<OversizeProps>();
</script>

<template>
  <ul :class="props.density">
    <li v-for="row in props.rows" :key="row.id">{{ row.label }}</li>
  </ul>
</template>
