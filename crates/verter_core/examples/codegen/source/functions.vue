<template>
  <div>
    <button @click="handleClick">Click me</button>
    <p>Count: {{ count }}</p>
  </div>
</template>
<script setup lang="ts">
import { ref } from 'vue'

const count = ref(0)

function handleClick() {
  count.value++
  processData(count.value)
}

function processData(value: number) {
  console.log('Processing:', value)
}

async function fetchData() {
  return await Promise.resolve('data')
}
</script>
