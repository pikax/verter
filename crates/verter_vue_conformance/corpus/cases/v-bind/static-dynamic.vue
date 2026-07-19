<script setup>
import { ref } from "vue"

const title = ref("Hello")
const disabled = ref(false)
</script>

<template>
  <button type="button" :title="title" :disabled="disabled">Go</button>
</template>
