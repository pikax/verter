<script setup lang="ts">
import SlotTypedHost from "./SlotTypedHost.vue";
</script>

<template>
  <!-- Live type errors on scoped-slot prop usage -->
  <SlotTypedHost>
    <template #header="{ title, count }">
      <!-- title is string: number methods must error -->
      <span>{{ title.toFixed(2) }}</span>
      <!-- count is number: string methods must error -->
      <span>{{ count.toUpperCase() }}</span>
    </template>
    <template #default="{ body, flag }">
      <!-- body is string -->
      <span>{{ body.toFixed(1) }}</span>
      <!-- flag is boolean -->
      <span>{{ flag.toUpperCase() }}</span>
    </template>
  </SlotTypedHost>
</template>
