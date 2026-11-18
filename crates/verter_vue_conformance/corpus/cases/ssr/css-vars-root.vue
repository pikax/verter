<script setup lang="ts">
import { ref } from "vue"
const color = ref("red")
</script>

<template>
  <p class="a">text</p>
</template>

<style>
.a {
  color: v-bind(color);
}
</style>
