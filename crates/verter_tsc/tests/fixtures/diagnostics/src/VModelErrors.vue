<script setup lang="ts">
import { ref } from "vue";

// TS2322 — number not assignable to Ref<string>
const strRef = ref<string>(42);

// TS2322 — boolean not assignable to Ref<number>
const numRef = ref<number>(false);

// TS2345 — wrong argument type
const arr = ref<string[]>("not-array");
</script>
<template>
  <div>{{ strRef }} {{ numRef }} {{ arr }}</div>
</template>
