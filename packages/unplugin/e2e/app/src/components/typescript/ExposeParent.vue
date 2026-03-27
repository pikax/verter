<script setup lang="ts">
import { ref } from "vue";
import ExposeDemo from "./ExposeDemo.vue";

const childRef = ref<InstanceType<typeof ExposeDemo> | null>(null);
const result = ref("");

function callExposed() {
  if (childRef.value) {
    childRef.value.increment();
    result.value = `count: ${childRef.value.getCount()}`;
  }
}
</script>

<template>
  <div data-testid="expose-parent">
    <button data-testid="call-exposed" @click="callExposed">Call Exposed</button>
    <span data-testid="expose-result">{{ result }}</span>
    <ExposeDemo ref="childRef" />
  </div>
</template>
