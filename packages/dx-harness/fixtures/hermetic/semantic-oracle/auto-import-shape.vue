<template>
  <output>{{ current }}</output>
</template>

<script setup lang="ts">
import { ref } from "vue";

const counter = ref(0); // @dx-anchor autoImport.ref

const current = counter.value; // @dx-anchor autoImport.value
</script>
