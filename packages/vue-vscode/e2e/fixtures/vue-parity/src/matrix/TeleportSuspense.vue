<script setup lang="ts">
import { defineAsyncComponent, Suspense } from "vue";

const AsyncBit = defineAsyncComponent(async () => ({
  template: "<span>async</span>",
}));
</script>

<template>
  <div>
    <Teleport to="body">
      <div class="teleport-marker">ported</div>
    </Teleport>
    <Suspense>
      <AsyncBit />
      <template #fallback>loading</template>
    </Suspense>
  </div>
</template>
