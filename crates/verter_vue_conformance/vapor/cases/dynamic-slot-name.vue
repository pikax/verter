<script setup>
import { ref } from 'vue'
const families = ref([])
const rows = ref([])
const name = ref('cell')
</script>
<template>
  <Table :rows="rows">
    <template #head="{ row }"><b>{{ row.name }}</b></template>
    <template v-for="family in families" #[`cell-${family}`]="{ row }">{{ row.versions[family] }}</template>
    <template #[name]="{ row }"><i>{{ row.id }}</i></template>
  </Table>
</template>
