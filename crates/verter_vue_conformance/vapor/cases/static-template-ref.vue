<script setup>
import { ref } from 'vue'
const label = ref('')
const items = ref([])
function close() {}
</script>
<template>
  <details ref="menu"><summary :aria-label="label"><Icon />{{ label }}</summary><div @click="close"><slot /></div></details>
  <Panel ref="panel" />
  <ul><li v-for="item in items" :key="item" ref="rows">{{ item }}</li></ul>
</template>
