<script setup>
import { provide, ref, readonly } from "vue";

const counter = ref(0);
const theme = ref("light");

// Provide values
provide("counter", readonly(counter));
provide("theme", theme.value);
provide("message", "Hello from provider");

function increment() {
  counter.value++;
}

function toggleTheme() {
  theme.value = theme.value === "light" ? "dark" : "light";
}
</script>

<template>
  <div>
    <p>Counter: {{ counter }}</p>
    <p>Theme: {{ theme }}</p>
    <button @click="increment">Increment</button>
    <button @click="toggleTheme">Toggle Theme</button>
    <slot />
  </div>
</template>
