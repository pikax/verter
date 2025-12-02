<script setup lang="ts">
// Lifecycle hooks patterns for parser testing

import {
  ref,
  onMounted,
  onUnmounted,
  onBeforeMount,
  onBeforeUnmount,
  onUpdated,
  onBeforeUpdate,
  onActivated,
  onDeactivated,
  onErrorCaptured,
  onRenderTracked,
  onRenderTriggered,
  type DebuggerEvent,
} from "vue";

const count = ref(0);

// Basic hooks
onBeforeMount(() => {
  console.log("beforeMount");
});

onMounted(() => {
  console.log("mounted");
});

onBeforeUpdate(() => {
  console.log("beforeUpdate");
});

onUpdated(() => {
  console.log("updated");
});

onBeforeUnmount(() => {
  console.log("beforeUnmount");
});

onUnmounted(() => {
  console.log("unmounted");
});

// Keep-alive hooks
onActivated(() => {
  console.log("activated");
});

onDeactivated(() => {
  console.log("deactivated");
});

// Error handling with typed parameters
onErrorCaptured((err: Error, instance, info: string): boolean | void => {
  console.log("error:", err.message, "info:", info);
  return false; // prevent propagation
});

// Debug hooks with typed event
onRenderTracked((event: DebuggerEvent) => {
  console.log("tracked:", event.key, event.type);
});

onRenderTriggered((event: DebuggerEvent) => {
  console.log("triggered:", event.key, event.type);
});

// Multiple hooks of same type
onMounted(() => {
  console.log("first mounted hook");
});

onMounted(() => {
  console.log("second mounted hook");
});

// Async hook
onMounted(async () => {
  await new Promise((r) => setTimeout(r, 100));
  console.log("async mounted");
});

// Hook with cleanup pattern
onMounted(() => {
  const handler = () => console.log("resize");
  window.addEventListener("resize", handler);
  
  onUnmounted(() => {
    window.removeEventListener("resize", handler);
  });
});

// Nested hook registration (valid in setup)
function useFeature() {
  onMounted(() => {
    console.log("feature mounted");
  });
  
  onUnmounted(() => {
    console.log("feature unmounted");
  });
}

useFeature();
</script>

<template>
  <div>
    <p>Count: {{ count }}</p>
    <button @click="count++">Increment</button>
  </div>
</template>
