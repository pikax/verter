<script setup>import { read } from './state'; let value = read();</script>
<template><input v-model="value">{{ value }}</template>
