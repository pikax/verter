<script setup lang="ts">
import { ref } from 'vue'
const groups = ref([{ children: [1] }])
const fallback = ref([2])
</script>
<template><section v-for="group in groups"><div v-for="group in (group.children || fallback).filter(group => group > 0)">{{ group }}</div></section></template>
