<template>
  <div :class="$style.container">
    <h1 :class="$style.title">CSS Module Test</h1>
    <p :class="[$style.text, $style.highlight]">Multiple classes from module</p>
    <button :class="{ [$style.button]: true, [$style.active]: isActive }">
      Dynamic Module Class
    </button>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'

const isActive = ref(false)
</script>

<style module>
.container {
  padding: 20px;
  background: white;
}

.title {
  color: navy;
  font-size: 24px;
}

.text {
  color: #444;
}

.highlight {
  background: yellow;
}

.button {
  padding: 8px 16px;
  border: 1px solid #ccc;
  cursor: pointer;
}

.active {
  background: #007bff;
  color: white;
}
</style>
