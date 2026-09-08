<script setup lang="ts" generic="T extends Identified">
// Fixed workload fixture. Open-generic carrier: the props surface stays a
// shallow carrier (open key domain), so this fixture drives the shallow /
// carrier-stop path rather than whole-surface materialisation. Frozen bytes.
import type { Emitted, Identified, PropsBase, Selection } from "../shared/props-base";
import type { ThemeSlice } from "../shared/theme";

const props = defineProps<PropsBase<T> & ThemeSlice<"accent">>();
const emit = defineEmits<Emitted<T>>();

defineSlots<{ row(scope: { row: T }): unknown }>();

function current(): Selection<T>["selected"] {
  return props.selected ?? null;
}
</script>

<template>
  <table :data-density="props.density">
    <tbody>
      <tr v-for="row in props.rows" :key="row.id" @click="emit('select', row)">
        <slot name="row" :row="row" />
      </tr>
    </tbody>
    <caption v-if="current()">{{ props.accent }}</caption>
  </table>
</template>
