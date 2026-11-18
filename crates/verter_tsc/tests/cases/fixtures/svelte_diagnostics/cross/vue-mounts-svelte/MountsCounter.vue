<script setup lang="ts">
import { mount } from "svelte";
import Counter from "./Counter.svelte";
mount(Counter, {
  target: document.body,
  props: { count: "x" },
});
</script>

<template>
  <p>mounted</p>
</template>
