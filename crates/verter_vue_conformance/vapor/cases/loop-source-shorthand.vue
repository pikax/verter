<script setup lang="ts">
import { ref } from 'vue'
const rows = ref([1, 2])
const select = (input: { rows: number[] }) => input.rows
</script>
<template><div v-for="row in select({ rows })">{{ row }}</div></template>
