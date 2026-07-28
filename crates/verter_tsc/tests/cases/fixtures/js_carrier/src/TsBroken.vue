<script setup lang="ts">
const n: number = 'not a number'

function takesNumber(x: number) {
  return x
}

takesNumber('also wrong')
</script>
<template><div>{{ n }}{{ takesNumber(1) }}</div></template>
