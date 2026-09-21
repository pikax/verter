<script setup lang="ts">
import GenericSlotSource from "./GenericSlotSource.vue";

defineProps<{ value: string }>();
const item = "parent";
const rows = [1, 2];
</script>

<template>
  <GenericSlotSource :value="value" v-slot="{ value }">
    {{ value.toUpperCase() }}
  </GenericSlotSource>
  <GenericSlotSource :value="item">
    <template v-for="item in rows" #default="{ value: parentValue }">
      {{ parentValue.toUpperCase() }} {{ item.toFixed() }}
    </template>
  </GenericSlotSource>
</template>
