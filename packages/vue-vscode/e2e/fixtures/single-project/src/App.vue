<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import MyComp from './MyComp.vue'
import { formatCount } from './utils'

const count = ref(0)
const doubled = computed(() => count.value * 2)
const props = defineProps<{ title: string }>()
const formatted = formatCount(count.value)

onMounted(() => { console.log('mounted') })
watch(count, (val) => { console.log(val) })

function increment() { count.value++ }
</script>
<template>
  <div>
    <h1>{{ title }}</h1>
    <p>{{ count }} x 2 = {{ doubled }}</p>
    <p>{{ formatted }}</p>
    <button @click="increment">+</button>
    <MyComp foo="literal" :bar="count" />
  </div>
</template>
