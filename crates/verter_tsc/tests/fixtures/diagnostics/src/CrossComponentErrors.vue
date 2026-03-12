<script setup lang="ts">
import type { User, Status } from './types'

// TS2322 — wrong types for User fields
const user: User = {
  id: true,
  name: 42,
  email: null,
  age: 'old',
}

// TS2322 — invalid status value
const status: Status = 'unknown'

// TS2322 — number not assignable to string
const name: string = 100
</script>
<template>
  <div>{{ user.name }} {{ status }} {{ name }}</div>
</template>
