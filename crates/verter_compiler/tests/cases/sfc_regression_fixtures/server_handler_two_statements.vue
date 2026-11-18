<script setup lang="ts">
import { ref } from 'vue';
const a = ref(''), b = ref('');
</script>
<template><div><button @click="a = ''; b = ''">x</button></div></template>
