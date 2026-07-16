<script setup lang="ts">
import { computed } from "vue";
import { ref } from "vue";

const count = ref(0);
const doubled = computed(() => count.value * 2);
</script>

<template>
  <p>{{ count }} / {{ doubled }}</p>
</template>
