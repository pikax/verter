<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import HelloWorld from '@/components/HelloWorld.vue'
import { double } from '@/utils/math'

const count = ref(0)
const doubled = computed(() => double(count.value))
const props = defineProps<{ title: string }>()

onMounted(() => { console.log('mounted') })
watch(count, (val) => { console.log(val) })

function increment() { count.value++ }
</script>
<template>
  <div>
    <h1>{{ title }}</h1>
    <p>{{ count }} x 2 = {{ doubled }}</p>
    <button @click="increment">+</button>
    <HelloWorld msg="hello" />
  </div>
</template>
