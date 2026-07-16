<script setup lang="ts">
import { ref } from "vue";

const attrName = ref("title");
const attrValue = ref("dynamic");
</script>

<template>
  <div>
    <span :[attrName]="attrValue">dyn</span>
    <button @[attrName]="() => {}">evt</button>
  </div>
</template>
