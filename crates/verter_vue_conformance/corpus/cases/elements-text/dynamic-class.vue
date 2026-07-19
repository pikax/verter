<script setup>
import { ref } from "vue"

const active = ref(true)
const size = ref("lg")
</script>

<template>
  <div :class="['card', { active }, 'size-' + size]">Classy</div>
</template>
