<script setup>
import { ref } from "vue"

const color = ref("red")
const top = ref(10)
</script>

<template>
  <div :style="{ color, marginTop: top + 'px' }">Styled</div>
</template>
