<script setup lang="ts">
import { ref } from 'vue'
const rows = ref([{ active: true }])
const enabled = ref(true)
</script>
<template><div v-for="row in rows.filter(row => row.active && enabled)">{{ row }}</div></template>
