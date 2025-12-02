<script setup>
// defineOptions patterns for parser testing - Setup API + JavaScript

import { ref } from "vue";

// Comprehensive defineOptions usage
defineOptions({
  // Component name for debugging and recursive components
  name: "DefineOptionsSetupJs",

  // Disable attribute inheritance
  inheritAttrs: false,

  // Custom options for third-party plugins
  customOptions: {
    // For route meta (vue-router plugin)
    routeMeta: {
      requiresAuth: true,
      roles: ["admin", "user"],
    },
    // For i18n plugin
    i18n: {
      messages: {
        en: { greeting: "Hello" },
        es: { greeting: "Hola" },
      },
    },
    // Custom plugin options
    permission: "admin",
    feature: "beta",
  },
});

const message = ref("Hello from defineOptions component");
const count = ref(0);

function increment() {
  count.value++;
}
</script>

<template>
  <div>
    <p>{{ message }}</p>
    <p>Count: {{ count }}</p>
    <button @click="increment">Increment</button>
  </div>
</template>
