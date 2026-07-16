<script setup lang="ts">
import DirectChild from "./components/DirectChild.vue";
const directLabel = "direct";
</script>

<template><DirectChild :contract-prop="directLabel" /></template>
