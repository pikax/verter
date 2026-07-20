<script setup>
import { ref } from "vue"

const plain = ref("plain text")
</script>

<template>
  <p v-text="plain"></p>
</template>
