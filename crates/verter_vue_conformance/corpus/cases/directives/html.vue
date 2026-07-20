<script setup>
import { ref } from "vue"

const raw = ref("<b>bold</b>")
</script>

<template>
  <div v-html="raw"></div>
</template>
