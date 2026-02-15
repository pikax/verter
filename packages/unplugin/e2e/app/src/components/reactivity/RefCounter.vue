<script setup lang="ts">
import { ref, computed, watch } from 'vue'

const count = ref(0)
const doubled = computed(() => count.value * 2)
const watchLog = ref<string[]>([])

watch(count, (newVal, oldVal) => {
  watchLog.value.push(`${oldVal} → ${newVal}`)
})

function increment() {
  count.value++
}
</script>

<template>
  <div data-testid="ref-counter">
    <span data-testid="count">{{ count }}</span>
    <span data-testid="doubled">{{ doubled }}</span>
    <button data-testid="increment" @click="increment">Increment</button>
    <ul data-testid="watch-log">
      <li v-for="(entry, i) in watchLog" :key="i">{{ entry }}</li>
    </ul>
  </div>
</template>
