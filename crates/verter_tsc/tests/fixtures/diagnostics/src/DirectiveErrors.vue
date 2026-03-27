<script setup lang="ts">
import type { Directive } from "vue";

const vColor: Directive<HTMLElement, string, "red" | "blue"> = (el, binding) => {
  el.style.color = binding.value;
};
</script>
<template>
  <span v-color.blue="'valid'" />
  <span v-color.green="'invalid-modifier'" />
  <span v-color.blue="123" />
</template>
