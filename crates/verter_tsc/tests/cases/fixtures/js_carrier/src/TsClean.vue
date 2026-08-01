<script setup lang="ts">
import { ref } from 'vue'

const props = defineProps<{ label: string }>()
const count = ref(0)

function bump(step: number) {
  count.value += step
}
</script>
<template><button @click="bump(1)">{{ props.label }} {{ count }}</button></template>
