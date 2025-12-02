<script>
import { ref, readonly } from "vue";

export default {
  name: "ProvideInject",
  provide() {
    return {
      counter: readonly(this.counterRef),
      theme: this.theme,
      message: "Hello from provider",
    };
  },
  data() {
    return {
      counterRef: ref(0),
      theme: "light",
    };
  },
  computed: {
    counter() {
      return this.counterRef.value;
    },
  },
  methods: {
    increment() {
      this.counterRef.value++;
    },
    toggleTheme() {
      this.theme = this.theme === "light" ? "dark" : "light";
    },
  },
};
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
