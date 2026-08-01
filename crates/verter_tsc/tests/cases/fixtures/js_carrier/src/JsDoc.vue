<script setup>
/** @param {number} n */
function timesTwo(n) {
  return n * 2
}

const bad = timesTwo('nope')
</script>
<template><div>{{ bad }}</div></template>
