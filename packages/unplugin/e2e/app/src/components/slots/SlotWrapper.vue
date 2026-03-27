<script setup lang="ts">
import { ref } from "vue";

const scopedCount = ref(0);

function incrementScoped() {
  scopedCount.value++;
}
</script>

<template>
  <div data-testid="slot-wrapper">
    <header data-testid="slot-header">
      <slot name="header" />
    </header>
    <main data-testid="slot-body">
      <slot />
    </main>
    <footer data-testid="slot-footer">
      <slot name="footer" />
    </footer>
    <div data-testid="slot-scoped-area">
      <slot name="scoped" :count="scopedCount" :increment="incrementScoped" />
    </div>
  </div>
</template>
