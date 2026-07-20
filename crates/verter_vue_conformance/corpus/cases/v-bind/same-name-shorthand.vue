<script setup>
import { ref } from "vue"

const id = ref("a1")
const title = ref("Hi")
</script>

<template>
  <div :id :title></div>
</template>
