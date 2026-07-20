<script setup>
import { ref } from "vue"

const scores = ref({ alice: 3, bob: 5 })
</script>

<template>
  <ul>
    <li v-for="(score, name) in scores" :key="name">{{ name }}: {{ score }}</li>
  </ul>
</template>
