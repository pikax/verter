<script setup lang="ts">
import GenericList from "./GenericList.vue";

interface Row {
  id: string;
  label: string;
}

const rows: Row[] = [
  { id: "1", label: "one" },
  { id: "2", label: "two" },
];
</script>

<template>
  <GenericList :items="rows" selected-id="1" />
</template>
