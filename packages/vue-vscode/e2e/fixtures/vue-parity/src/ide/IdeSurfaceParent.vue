<script setup lang="ts">
import IdeSurfaceChild from "./IdeSurfaceChild.vue";

const label = "ide-label";
const count = 2;

function onPick(value: string): void {
  void value;
}

function onChange(next: number): void {
  void next;
}

interface IdePerson {
  name: string;
  age: number;
}

const person: IdePerson | null = { name: "Ada", age: 36 };
</script>

<template>
  <div>
    <!-- PROP_ATTR_SITE: Ctrl+Click label → child prop; completion after space on tag -->
    <IdeSurfaceChild
      :label="label"
      :count="count"
      @pick="onPick"
      @change="onChange"
    >
      <!-- SLOT_HEADER_SITE -->
      <template #header="{ title, count: slotCount }">
        <span>{{ title }}:{{ slotCount }}</span>
      </template>
      <template #default="{ body }">
        <p>{{ body }}</p>
      </template>
    </IdeSurfaceChild>

    <!-- EVENT_HANDLER_COMPLETE_SITE: type after @pick=" -->
    <!-- used by tests via edit; keep a stable second usage -->
    <IdeSurfaceChild :label="label" :count="count" @pick="onPick" />

    <!-- NARROW_SITE -->
    <div v-if="person">
      <span>{{ person.name }}</span>
    </div>

    <!-- PROP_COMPLETE_SITE -->
    <!-- SLOT_NAME_COMPLETE_SITE -->
    <!-- DIRECTIVE_COMPLETE_SITE -->
    <!-- EVENT_ATTR_COMPLETE_SITE -->
  </div>
</template>
