<script setup>
/** @template T @param {T} value @returns {T} */
const identity = (value) => value
const label = identity('label')
</script>
<template><div>{{ label }}</div></template>
