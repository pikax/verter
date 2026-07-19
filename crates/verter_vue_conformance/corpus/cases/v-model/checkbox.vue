<script setup>
import { ref } from "vue"

const agreed = ref(false)
</script>

<template>
  <label><input type="checkbox" v-model="agreed" /> Agree</label>
</template>
