<script setup>
import { ref } from 'vue'
const groups = ref([])
const seen = ref({})
function keyOf(group) { return group }
</script>
<template>
  <div v-for="group in groups" :key="keyOf(group)">
    <p v-for="item in group.items" :key="`${group.id}/${item.id}/${seen[item.id] ?? 0}`">{{ item.id }}</p>
    <i v-for="group in group.children" :key="group.id + seen.base">{{ group.id }}</i>
  </div>
</template>
