<script setup>
import { ref } from "vue"
import ChildComp from "./child-comp.vue"

const current = ref(ChildComp)
</script>

<template>
  <component :is="current" label="Dynamic" />
</template>
