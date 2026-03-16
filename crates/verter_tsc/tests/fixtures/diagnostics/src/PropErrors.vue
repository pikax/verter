<script setup lang="ts">
import type { User } from './types'

// TS2322 — wrong field types on a typed object
const user: User = {
  id: 'not-a-number',
  name: 123,
  email: true,
  age: 'old',
}

// TS2322 — string not assignable to number
const count: number = 'five'
// TS2322 — boolean not assignable to string
const label: string = true
</script>
<template>
  <!-- PINNED: line 18 — template renders user -->
  <div>{{ user.name }} {{ count }} {{ label }}</div>
</template>
