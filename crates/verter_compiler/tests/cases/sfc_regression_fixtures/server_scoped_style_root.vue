<script setup lang="ts">
import { ref } from 'vue';
const n = ref(1);
</script>
<template><p class="a">{{ n }}</p></template>
<style scoped>
.a{color:red}
</style>
