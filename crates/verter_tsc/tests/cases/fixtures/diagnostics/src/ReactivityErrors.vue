<script setup lang="ts">
import { ref, reactive, watch } from "vue";
import type { User } from "./types";

// TS2322 — number not assignable to string[]
const tags = ref<string[]>(42);

// TS2322 — wrong fields in reactive
const user = reactive<User>({
  id: "not-number",
  name: 42,
  email: false,
  age: "old",
});

// TS2345 — watch callback argument type
watch(tags, (newVal) => {
  // newVal is string[], calling toFixed is wrong
  const bad: number = newVal;
});
</script>
<template>
  <div>{{ user.name }}</div>
</template>
