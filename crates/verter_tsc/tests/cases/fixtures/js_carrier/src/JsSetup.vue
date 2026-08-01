<script setup>
import { ref, computed } from 'vue'

const props = defineProps({ label: { type: String, required: true } })
const emit = defineEmits(['change'])

const count = ref(0)
const doubled = computed(() => count.value * 2)

function bump(step) {
  count.value += step
  emit('change', count.value)
}

function describe(item) {
  return `${props.label}: ${item.name}`
}
</script>
<template>
  <button @click="bump(1)">{{ label }} {{ doubled }}</button>
  <span>{{ describe({ name: 'x' }) }}</span>
</template>
