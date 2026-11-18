<script setup>import { ref } from 'vue'; const rows = ref([{ id: 1, label: 'a' }]); function pick(row) {}</script>
<template><div><div v-for="row in rows" :key="row.id"><button @click="pick(0)" v-on:click="pick(row)">{{ row.label }}</button></div></div></template>
