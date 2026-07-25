<script setup lang="ts">
import { ref } from "vue";

/** Hovering this must report `string` — a typed answer, from THIS package's TypeScript. */
const packageLocalLabel: string = "served from packages/app";

const counter = ref(0);

/** A deliberate type error: `strict` is this project's own rule, from its own config. */
const mismatched: number = "not a number";
</script>

<template>
  <button @click="counter++">{{ packageLocalLabel }} {{ counter }} {{ mismatched }}</button>
</template>
