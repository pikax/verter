<script setup>
import { ref } from "vue"

const items = ref(["a", "b", "c"])
</script>

<template>
  <ul>
    <li v-for="item in items" :key="item">{{ item }}</li>
  </ul>
</template>
