<script setup>
import { ref } from "vue"

const attrName = ref("title")
const value = ref("Tooltip")
</script>

<template>
  <p :[attrName]="value">Dynamic attribute</p>
</template>
