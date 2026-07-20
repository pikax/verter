<script setup>
import { ref } from "vue"

const picked = ref("a")
</script>

<template>
  <select v-model="picked">
    <option value="a">A</option>
    <option value="b">B</option>
  </select>
</template>
