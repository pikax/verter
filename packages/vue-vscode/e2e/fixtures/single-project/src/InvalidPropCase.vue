<script setup lang="ts">
import MyComp from "./MyComp.vue";
</script>
<template>
  <MyComp foo="valid" :bar="42" thisDoesNotExist="bad" />
</template>
