<script setup>
import { ref } from "vue"

const users = ref([
  { id: 1, name: "Ada" },
  { id: 2, name: "Bo" },
])
</script>

<template>
  <ul>
    <li v-for="({ id, name }, index) in users" :key="id">
      {{ index }} — {{ name }}
    </li>
  </ul>
</template>
