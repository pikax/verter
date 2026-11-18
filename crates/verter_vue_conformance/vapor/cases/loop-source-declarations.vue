<script setup lang="ts">
import { ref } from 'vue'
const local = ref([2])
</script>
<template><div v-for="x in (() => { const result = local(); function local() { return [1] } return result })()">{{ x }}</div><div v-for="x in (() => { { function local() { return [1] } } return local })()">{{ x }}</div></template>
