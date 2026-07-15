<script setup lang="ts">
import { ref } from "vue";

// CTRL+CLICK targets: both `key` (the computed attribute name) and `val`
// (the bound value) in `:[key]="val"` must navigate to their declarations here.
const key = ref("title");
const val = ref("a value");
</script>
<template>
  <div :[key]="val" />
</template>
