<script setup lang="ts">
import { mount } from "svelte";
import Counter from "./Counter.svelte";
mount(Counter, {
  target: document.body,
  props: { count: 1 },
});
</script>

<template>
  <p>mounted</p>
</template>
