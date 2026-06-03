<script setup lang="ts">
import { ref } from "vue";

// CTRL+CLICK target: `count` in native `v-model="count"` is emitted as both a
// read occurrence (`value={count}`) and an assignment occurrence inside the
// `onInput` handler. EVERY generated occurrence must navigate back here.
const count = ref(0);
</script>
<template>
  <input v-model="count" />
  <input v-model.trim="count" />
</template>
