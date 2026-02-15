<script lang="ts">
import { defineComponent } from 'vue'

export default defineComponent({
  data() {
    return {
      count: 0,
      watchHistory: [] as string[],
    }
  },
  computed: {
    doubled(): number {
      return this.count * 2
    },
  },
  watch: {
    count(newVal: number, oldVal: number) {
      this.watchHistory.push(`${oldVal} → ${newVal}`)
    },
  },
  methods: {
    increment() {
      this.count++
    },
  },
})
</script>

<template>
  <div data-testid="options-counter">
    <span data-testid="options-count">{{ count }}</span>
    <span data-testid="options-doubled">{{ doubled }}</span>
    <button data-testid="options-increment" @click="increment">Increment</button>
    <ul data-testid="options-watch-history">
      <li v-for="(entry, i) in watchHistory" :key="i">{{ entry }}</li>
    </ul>
  </div>
</template>
