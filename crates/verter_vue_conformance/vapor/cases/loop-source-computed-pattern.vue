<script setup lang="ts">
import { ref } from 'vue'
const rows = ref([{ name: 'one' }])
const field = ref('name')
const table = ref({ name: [1] })
</script>
<template><div v-for="x in rows.map(({ [field]: value }) => value)">{{ x }}</div><div v-for="x in (() => { const { [field]: entries } = table; return entries })()">{{ x }}</div></template>
