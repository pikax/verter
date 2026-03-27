<script setup lang="ts">
import { ref } from "vue";

const active = ref(false);
</script>
<template>
  <div class="container">
    <p class="title" id="main-title">Hello</p>
    <span :class="{ active }">Toggle</span>
  </div>
</template>
<style scoped>
.container {
  display: flex;
}
.title {
  font-size: 1.5rem;
}
#main-title {
  color: blue;
}
.active {
  background: green;
}
</style>
