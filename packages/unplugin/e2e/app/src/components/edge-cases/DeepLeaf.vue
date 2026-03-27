<script setup lang="ts">
import { inject } from "vue";
import type { Ref } from "vue";

const deepValue = inject<Ref<string>>("deep-value");
const updateRoot = inject<() => void>("update-root");
</script>

<template>
  <div data-testid="deep-leaf">
    <span data-testid="deep-leaf-value">{{ deepValue }}</span>
    <button data-testid="deep-leaf-btn" @click="updateRoot">Update Root</button>
  </div>
</template>
