<script setup>
import { ref } from 'vue'
const label = ref('')
const items = ref([])
const busy = ref(false)
const deep = ref('')
function go() {}
</script>
<template>
  <figure>
    <div :aria-label="label"><span v-for="item in items" :key="item">{{ item }}</span></div>
    <p><button :disabled="busy" @click="go">go</button><b>{{ label }}</b></p>
    <section><article :title="label"><em>{{ deep }}</em></article></section>
  </figure>
</template>
