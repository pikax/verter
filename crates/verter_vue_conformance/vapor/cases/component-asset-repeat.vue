<script setup>
import { ref } from 'vue'
const rows = ref([])
const on = ref(false)
</script>
<template>
  <div>
    <Badge v-if="on" />
    <Badge v-else tone="x" />
    <Badge v-for="row in rows" :key="row" :tone="row" />
    <Badge />
    <Badge />
  </div>
</template>
