<script setup lang="ts">
// Globally-registered component (see global-components.d.ts). It is NEVER imported by a
// consumer; the IDE codegen synthesizes a `GlobalComponents` fallback const so the
// template event-typing path can read `InstanceType<typeof GlobalEmitComp>["$props"]`.
defineProps<{
  onPing?: (payload: { pingCode: string; pingCount: number }) => void;
  "onLate-signal"?: (sig: { sigName: string; sigLevel: number }) => void;
}>();
</script>

<template>
  <div>global emit comp</div>
</template>
