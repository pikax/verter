<script setup>
import { ref } from "vue"

const open = ref(true)
</script>

<template>
  <Teleport to="body" :disabled="!open">
    <p class="overlay">Overlay</p>
  </Teleport>
</template>
