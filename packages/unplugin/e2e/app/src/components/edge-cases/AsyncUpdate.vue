<script setup lang="ts">
import { ref } from "vue";

const status = ref("idle");

function triggerAsync() {
  status.value = "loading";
  setTimeout(() => {
    status.value = "done";
  }, 100);
}
</script>

<template>
  <div data-testid="async-update">
    <span data-testid="async-status">{{ status }}</span>
    <button data-testid="async-trigger" @click="triggerAsync">Trigger</button>
  </div>
</template>
