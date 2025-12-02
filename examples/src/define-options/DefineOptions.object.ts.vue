<script lang="ts">
// defineOptions patterns for parser testing - Options API + TypeScript
// Note: defineOptions is primarily for script setup. In Options API, 
// these options are set directly in the component definition.

import { defineComponent } from "vue";

export default defineComponent({
  // Component name for debugging and recursive components
  name: "DefineOptionsObjectTs",

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

  data() {
    return {
      message: "Hello from options API component" as string,
      count: 0 as number,
    };
  },

  methods: {
    increment(): void {
      this.count++;
    },
  },
});
</script>

<template>
  <div>
    <p>{{ message }}</p>
    <p>Count: {{ count }}</p>
    <button @click="increment">Increment</button>
    <!-- Manual $attrs forwarding since inheritAttrs is false -->
    <div v-bind="$attrs">Attrs forwarded here</div>
  </div>
</template>
