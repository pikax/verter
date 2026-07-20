<script setup>
import { ref } from "vue"
import ChildComp from "../components/child-comp.vue"

const current = ref(ChildComp)
</script>

<template>
  <KeepAlive :max="2">
    <component :is="current" label="Cached" />
  </KeepAlive>
</template>
