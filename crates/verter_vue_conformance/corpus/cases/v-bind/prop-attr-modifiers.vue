<script setup>
import { ref } from "vue"

const text = ref("inner")
const dataX = ref("x")
</script>

<template>
  <div :text-content.prop="text" :data-x.attr="dataX"></div>
</template>
