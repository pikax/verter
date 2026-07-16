<script setup lang="ts">
import type { ConfidenceItem } from "./CrossFileTypes";
import GenericList from "../generics/GenericList.vue";

const good: ConfidenceItem[] = [{ id: "a", label: "A", count: 1 }];
// Wrong element shape for GenericList constraint T extends { id: string } is ok,
// but count must remain number when used as ConfidenceItem.
const badItems = [{ id: 1, label: "x" }] as unknown as ConfidenceItem[];
</script>

<template>
  <!-- good path -->
  <GenericList :items="good" />
  <!-- Live error path: force wrong selected-id type if typed as string -->
  <GenericList :items="good" :selected-id="99" />
  <!-- Keep badItems referenced so the binding stays live for edits -->
  <span>{{ badItems[0]?.label }}</span>
</template>
