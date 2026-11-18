<script setup>import { ref } from 'vue'; const rows = ref([1, 2]); const draft = ref({}); function pick(row) {}</script>
<template><div><div v-for="row in rows" :key="row"><button @click="pick(row)">x</button><button @click="pick(0)">y</button><input v-model="draft[row]"></div></div></template>
