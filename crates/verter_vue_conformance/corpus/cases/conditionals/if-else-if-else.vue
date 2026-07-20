<script setup>
import { ref } from "vue"

const status = ref("loading")
</script>

<template>
  <p v-if="status === 'loading'">Loading</p>
  <p v-else-if="status === 'error'">Failed</p>
  <p v-else>Done: {{ status }}</p>
</template>
