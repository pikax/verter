<script setup lang="ts">
const heading = "scoped";
</script>

<template>
  <section class="card" id="card-root">
    <h2 class="card-title">{{ heading }}</h2>
    <p class="card-body">body</p>
  </section>
</template>

<style scoped>
.card {
  display: block;
}
.card-title {
  font-weight: 600;
}
#card-root {
  color: inherit;
}
</style>
