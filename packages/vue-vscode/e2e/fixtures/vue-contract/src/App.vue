<script setup lang="ts">
import DirectChild from "./components/DirectChild.vue";

interface ContractValue {
  label: string;
  count: number;
}

const typedValue: ContractValue = { label: "typed", count: 1 };
function renderTyped(): string {
  return `${typedValue.label}:${typedValue.count}`;
}
</script>

<template>
  <main>
    <button @click="renderTyped">{{ typedValue.label }}</button>
    <DirectChild :contract-prop="typedValue.label" />
  </main>
</template>
