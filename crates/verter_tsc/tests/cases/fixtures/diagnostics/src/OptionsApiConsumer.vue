<script setup lang="ts">
import OptionsApiAdvanced from "./OptionsApiAdvanced.vue";
</script>
<template>
  <OptionsApiAdvanced :initial-count="42" />
  <OptionsApiAdvanced :initial-count="'wrong'" />
</template>
