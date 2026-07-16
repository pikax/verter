<script setup lang="ts">
import SlotTypedHost from "./SlotTypedHost.vue";
</script>

<template>
  <!-- Unknown / mistyped slot names and missing required slot surface -->
  <SlotTypedHost>
    <!-- unknown slot name: should error or be flagged when strict slots / TS check applies -->
    <template #doesNotExist="{ x }">
      <span>{{ x }}</span>
    </template>
    <!-- header slot props: access undeclared slot prop binding -->
    <template #header="{ title }">
      <span>{{ title }}</span>
      <span>{{ missingSlotProp }}</span>
    </template>
  </SlotTypedHost>
</template>
