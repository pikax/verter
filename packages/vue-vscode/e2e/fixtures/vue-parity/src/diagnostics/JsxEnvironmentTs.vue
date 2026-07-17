<script setup lang="ts">
declare const Child: new () => { $props: { label: string } };
const doubled: number = 2;
</script>

<template>
  <div class="card">
    {{ doubled }}
    <Child label="ok">slot content</Child>
  </div>
</template>

<style scoped>
.card {
  color: red;
}
</style>
