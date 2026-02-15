<script setup>
import { ref } from 'vue'
const msg = ref('Hello')
const count = ref(0)
</script>

<template>
  <div class="app">
    <h1>{{ msg }}</h1>
    <p>Count: {{ count }}</p>
    <button @click="count++">Increment</button>
  </div>
</template>

<style scoped>
.app {
  padding: 20px;
}
</style>
