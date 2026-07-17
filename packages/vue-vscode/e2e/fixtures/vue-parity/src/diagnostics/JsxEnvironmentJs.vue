<script setup>
/** @type {new () => { $props: { label: string } }} */
const Child = class {
  constructor() {
    this.$props = { label: "" };
  }
};
/** @type {number} */
const doubled = 2;
</script>

<template>
  <div class="card">
    {{ doubled }}
    <Child label="ok">slot content</Child>
  </div>
</template>
