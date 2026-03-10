<script setup lang="ts">
import { ref } from 'vue'
import MyComp from './MyComp.vue'

const count = ref(1)

function safeAction() {
  count.value++
}

const broken =
</script>

<template>
  <div>
    <p>{{ count }}</p>
    <button @click="safeAction">go</button>
    <MyComp foo="ok" :bar="count" />
  </div>
</template>
