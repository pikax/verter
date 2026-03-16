<script setup lang="ts">
import type { PaginatedResult, User } from './types'

// TS2322 — wrong type for PaginatedResult fields
const result: PaginatedResult<User> = {
  items: 'not-an-array',
  total: 'not-a-number',
  page: false,
}

// TS2344/TS2322 — generic constraint violations
function getFirst<T extends { id: number }>(items: T[]): T {
  return items[0]
}

// TS2345 — objects missing 'id' field
const first = getFirst([{ name: 'x' }])
</script>
<template>
  <div>{{ result.total }} {{ first }}</div>
</template>
