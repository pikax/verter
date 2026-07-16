<script setup lang="ts">
import { ref } from "vue";
import SlotHost from "./SlotHost.vue";
import EmitHost from "./EmitHost.vue";

const last = ref("");
function onPick(v: string) {
  last.value = v;
}
</script>

<template>
  <div>
    <SlotHost>
      <template #header="{ title }">
        <h3>{{ title }}</h3>
      </template>
      <template #default="{ body }">
        <p>{{ body }}</p>
      </template>
    </SlotHost>
    <EmitHost @pick="onPick" @close="() => {}" />
    <span>{{ last }}</span>
  </div>
</template>
