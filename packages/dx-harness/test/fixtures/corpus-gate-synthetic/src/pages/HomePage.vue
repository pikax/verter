<template>
  <main>
    <BaseCard>
      <template #title>{{ heading }}</template>
    </BaseCard>
    <WidgetList :items="rows" @select="onSelect" />
  </main>
</template>

<script setup lang="ts">
import { BaseCard } from "widgets";
import WidgetList from "../components/widgets/WidgetList.vue";
import { useHomeStore } from "../stores";

const store = useHomeStore();
const heading = "Home";
const rows = store.rows;

function onSelect(id: string): void {
  store.pick(id);
}
</script>
