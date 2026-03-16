<script setup lang="ts">
import { ref, computed, reactive } from 'vue'
import type { User } from './types'

// PINNED: line 5, TS2322 — string not assignable to number
const count = ref<number>('hello')

// TS2322 — computed return type mismatch
const doubled = computed((): string => {
  return count.value * 2
})

// TS6133 — unused variable
const unusedVar = 'never used'

// TS2322 — wrong reactive type
const user = reactive<User>({
  id: 'not-number',
  name: 42,
  email: false,
  age: 'old',
})
</script>
<template>
  <div>{{ doubled }}</div>
</template>
