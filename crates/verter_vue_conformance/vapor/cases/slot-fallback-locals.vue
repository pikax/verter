<script setup>
import { ref } from 'vue'
const titleId = ref('')
const title = ref('')
const busy = ref(false)
</script>
<template>
  <dialog>
    <header><slot name="head" :title-id="titleId"><h2 :id="titleId">{{ title }}</h2></slot><span /></header>
    <footer><slot name="foot"><button :disabled="busy">{{ title }}</button><i>after</i></slot></footer>
  </dialog>
</template>
