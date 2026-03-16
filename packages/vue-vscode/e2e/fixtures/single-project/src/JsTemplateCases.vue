<script setup>
import { ref } from 'vue'

/** @type {import('vue').Ref<number>} */
const count = ref(0)

/** @type {{ label: string, done: boolean }} */
const state = { label: 'ready', done: false }

function increment() {
  count.value++
}
</script>

<template>
  <div>
    <p>{{ count }}</p>
    <p>{{ state.label }}</p>
    <button @click="increment">{{ state.done }}</button>
  </div>
</template>
