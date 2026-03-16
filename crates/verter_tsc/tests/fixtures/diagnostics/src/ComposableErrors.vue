<script setup lang="ts">
import { ref, type Ref } from 'vue'

function useCounter(): { count: Ref<number>; label: Ref<string> } {
  return { count: ref(0), label: ref('clicks') }
}

const { count, label } = useCounter()

// TS2322 — Ref<number> assigned to string
const bad: string = count.value
// TS2345 — string not assignable to number
count.value = 'not-a-number'
</script>
<template>
  <div>{{ label }}: {{ count }}</div>
</template>
