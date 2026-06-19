<script setup lang="ts">
// TS2305 — module has no exported member 'NonExistent'
import { type NonExistent } from "./types";

// TS2322 — wrong field types on User
import type { User } from "./types";

const badUser: User = {
  id: "not-a-number",
  name: 123,
  email: true,
  age: "old",
};
</script>
<template>
  <div>{{ badUser.name }}</div>
</template>
