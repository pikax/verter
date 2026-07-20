<script setup>
import { ref } from "vue"

const visible = ref(true)
</script>

<template>
  <p v-show="visible">Peekaboo</p>
</template>
