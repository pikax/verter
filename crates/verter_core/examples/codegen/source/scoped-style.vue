<template>
  <div class="container">
    <h1 class="title">Scoped Style Test</h1>
    <p class="text">This tests scoped CSS with data-v attribute injection</p>
    <ChildComponent class="child" />
  </div>
</template>

<script setup lang="ts">
import ChildComponent from './ChildComponent.vue'
</script>

<style scoped>
.container {
  padding: 20px;
  background: #f5f5f5;
}

.title {
  color: #333;
  font-size: 24px;
}

.text {
  color: #666;
}

/* Deep selector - targets child component internals */
:deep(.child-inner) {
  border: 1px solid #ccc;
}

/* Slotted selector - styles slot content */
:slotted(.slot-content) {
  margin: 10px;
}

/* Global selector - escapes scoping */
:global(.global-class) {
  font-weight: bold;
}
</style>
