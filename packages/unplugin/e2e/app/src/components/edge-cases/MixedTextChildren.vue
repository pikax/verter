<script setup lang="ts">
import { ref } from "vue";
const items = ref([
  { id: 1, name: "Alpha", badge: "A" },
  { id: 2, name: "Beta", badge: "B" },
]);
const label = ref("static");
</script>

<template>
  <div data-testid="mixed-text-static"><span>before</span>middle<span>after</span></div>
  <div data-testid="mixed-text-interp"><span>before</span>{{ label }}<span>after</span></div>
  <div data-testid="mixed-text-vfor">
    <div v-for="item in items" :key="item.id" :data-testid="'mixed-item-' + item.id">
      {{ item.name }} <span :data-testid="'mixed-badge-' + item.id">{{ item.badge }}</span>
    </div>
  </div>
</template>
