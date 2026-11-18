<script setup>let value = ''; function changed() {}</script>
<template><Widget v-model="value" @update:model-value="changed"/></template>
