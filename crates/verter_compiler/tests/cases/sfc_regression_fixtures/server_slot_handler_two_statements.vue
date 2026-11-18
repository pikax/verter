<script setup lang="ts">
import { ref } from 'vue';
import Child from './Child.vue';
const a = ref(''), b = ref('');
</script>
<template><Child><button @click="a = ''; b = ''">x</button></Child></template>
