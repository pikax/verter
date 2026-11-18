<script setup>
import { ref } from 'vue'
const a = ref(false)
const b = ref(false)
const y = ref('')
</script>
<template>
  <template v-if="a">
    <!-- first -->
    <span v-if="b">one</span>
    <!-- second -->
    <em v-if="y">two</em>
    <i v-else>three</i>
  </template>
  <p v-else>z</p>
</template>
