<template>
  <input ref="inputRef" />
</template>

<script setup lang="ts">
import { ref } from "vue";

const inputRef = ref<HTMLInputElement | null>(null);

const el = inputRef.value; // @dx-anchor ref.value
</script>
