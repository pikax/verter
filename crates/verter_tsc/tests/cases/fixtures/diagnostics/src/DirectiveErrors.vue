<script setup lang="ts">
import type { Directive } from "vue";

const vColor: Directive<HTMLElement, string, "red" | "blue"> = (el, binding) => {
  el.style.color = binding.value;
};
const vTheme: Directive<HTMLElement, string, "blue" | "green", "fg" | "bg"> = (el, binding) => {
  el.style.color = binding.value;
};
</script>
<template>
  <span v-color.blue="'valid'" />
  <span v-color.green="'invalid-modifier'" />
  <span v-color.blue="123" />
  <span v-theme:fg="'valid-arg'" />
  <span v-theme:bg.blue.green="'valid-arg-and-modifiers'" />
  <span v-theme.blue.green="'valid-modifiers-no-arg'" />
  <span v-theme:badarg="'invalid-arg'" />
  <span v-theme:fg.badmod="'invalid-modifier-with-arg'" />
  <span v-theme:bg.some-mod="'invalid-hyphenated-modifier-with-arg'" />
</template>
