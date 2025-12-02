<script setup lang="ts">
// defineOptions patterns for parser testing - Setup API + TypeScript

import { ref } from "vue";

// Comprehensive defineOptions usage
defineOptions({
  // Component name for debugging and recursive components
  name: "DefineOptionsSetupTs",

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

const message = ref<string>("Hello from defineOptions component");
const count = ref<number>(0);

function increment(): void {
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
