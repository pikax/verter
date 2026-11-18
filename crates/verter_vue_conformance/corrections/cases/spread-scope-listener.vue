<script setup>import { ref } from 'vue'; const attrs = ref({ id: 'a' }); const label = ref('x'); function setBox(el) {} function pick(item) {}</script>
<template><Shell v-slot="{ item }"><button v-bind="attrs" @click="pick(item)" :title="label">x</button><p v-bind="attrs" :ref="setBox">y</p></Shell></template>
