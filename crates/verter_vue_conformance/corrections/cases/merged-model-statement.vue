<script setup>let value = ''; let failure = null;</script>
<template><Shell><Widget v-model="value" @update:model-value="failure = null"/></Shell></template>
