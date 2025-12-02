<script setup lang="ts">
import { provide, ref, readonly, type InjectionKey } from "vue";

// Typed injection keys
export const CounterKey: InjectionKey<number> = Symbol("counter");
export const ThemeKey: InjectionKey<"light" | "dark"> = Symbol("theme");

const counter = ref(0);
const theme = ref<"light" | "dark">("light");

// Provide values
provide(CounterKey, readonly(counter));
provide(ThemeKey, theme.value);
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
