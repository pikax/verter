<script setup lang="ts">
import SlotTypedHost from "./SlotTypedHost.vue";
</script>

<template>
  <!-- Correct scoped-slot usage: must stay diagnostic-clean -->
  <SlotTypedHost>
    <template #header="{ title, count }">
      <h3>{{ title }}:{{ count.toFixed(0) }}</h3>
    </template>
    <template #default="{ body, flag }">
      <p>{{ body }}:{{ flag }}</p>
    </template>
    <template #footer="{ ok }">
      <span>{{ ok }}</span>
    </template>
  </SlotTypedHost>
</template>
