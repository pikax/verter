<script setup lang="ts">
import { ref } from "vue";

const label = ref("x");
const num = ref(1);
</script>

<template>
  <!-- Live type errors on native attrs / directive values -->
  <input :disabled="'not-boolean'" />
  <input :value="num" />
  <!-- v-html expects string-ish; object is wrong -->
  <div v-html="{ not: 'html' }"></div>
  <!-- v-text with object -->
  <span v-text="{ label }"></span>
  <!-- class binding ok with string; force wrong type via non-string object without toString intent -->
  <div :class="num"></div>
  <p>{{ label }}</p>
</template>
