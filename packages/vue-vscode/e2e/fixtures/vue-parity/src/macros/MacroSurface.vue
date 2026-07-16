<script setup lang="ts">
import { ref } from "vue";
import EmitChild from "./EmitChild.vue";
import ModelChild from "./ModelChild.vue";
import SlotChild from "./SlotChild.vue";
import WithDefaultsChild from "./WithDefaultsChild.vue";

const show = ref(true);
const picked = ref("");

function onPick(value: string) {
  picked.value = value;
}
</script>

<template>
  <div>
    <EmitChild @pick="onPick" @close="() => {}" />
    <ModelChild v-model:open="show" />
    <SlotChild v-slot="{ slotItem, slotIndex }">
      <p>{{ slotItem.name }} #{{ slotIndex }}</p>
    </SlotChild>
    <WithDefaultsChild title="defaults" />
    <p>{{ picked }}</p>
  </div>
</template>
