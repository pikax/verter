<template>
  <div class="card">
    <header><slot name="title"></slot></header>
    <section><slot name="body"></slot></section>
    <footer><slot name="footer"></slot></footer>
  </div>
</template>

<script setup lang="ts">
defineProps<{ elevated: boolean }>();
</script>

<style scoped>
.card {
  border: 1px solid black;
}
</style>
