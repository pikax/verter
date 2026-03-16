<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import SharedComp from '@monorepo/shared/src/SharedComp.vue'
import { helper } from '@monorepo/shared/src/utils'

const count = ref(0)
const doubled = computed(() => count.value * 2)
const props = defineProps<{ title: string }>()
const greeting = helper()

onMounted(() => { console.log('mounted') })
watch(count, (val) => { console.log(val) })

function increment() { count.value++ }
</script>
<template>
  <div>
    <h1>{{ title }}</h1>
    <p>{{ greeting }}</p>
    <p>{{ count }} x 2 = {{ doubled }}</p>
    <button @click="increment">+</button>
    <SharedComp foo="literal" :bar="count" />
  </div>
</template>
