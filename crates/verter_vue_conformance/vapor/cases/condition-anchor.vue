<script setup>
import { ref } from 'vue'
const title = ref('')
</script>
<template>
  <section :title="title">
    <header v-if="title"><b v-if="title">{{ title }}</b><span>x</span><slot name="actions" /></header>
    <div class="body"><slot /></div>
  </section>
</template>
