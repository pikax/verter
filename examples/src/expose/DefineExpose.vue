<script setup lang="ts">
import { ref } from "vue";

// defineExpose with various patterns
const internalState = ref("internal");
const count = ref(0);

function publicMethod(arg: string): number {
  return arg.length;
}

async function asyncMethod(): Promise<string> {
  return "async result";
}

const arrowMethod = (x: number, y: number): number => x + y;

// Expose with object literal
defineExpose({
  publicMethod,
  asyncMethod,
  arrowMethod,
  count,
  // Inline method
  inlineMethod: () => "inline",
  // Renamed export
  renamed: internalState,
  // Literal value
  version: "1.0.0" as const,
  // Complex type
  config: {
    nested: {
      value: 42,
    },
  },
});
</script>

<template>
  <div>
    <p>Internal: {{ internalState }}</p>
    <p>Count: {{ count }}</p>
  </div>
</template>
