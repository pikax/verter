<script setup>function pick(item) {}</script>
<template><Shell v-slot="{ item }"><button @click="pick(item)">x</button><button @click="pick(0)">y</button></Shell></template>
