<script setup>import { ref } from 'vue'; const groups = ref([[1, 2]]);</script>
<template><div><div v-for="(row, i) in groups" :key="i"><span v-for="cell in row" :key="cell">{{ cell }}</span></div></div></template>
