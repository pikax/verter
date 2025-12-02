<script setup lang="ts">
// Heavy component stub - simulates a large component for preloading tests
import { ref } from "vue";

const loaded = ref(true);
</script>

<template>
  <div class="heavy-component">
    <p>Heavy Component (loaded: {{ loaded }})</p>
  </div>
</template>
