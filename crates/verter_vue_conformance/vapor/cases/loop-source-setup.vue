<script setup lang="ts">
import { ref } from 'vue'
const rows = ref([1, 2, 3])
</script>
<template><div v-for="row in rows.slice(0, 2)">{{ row }}</div></template>
