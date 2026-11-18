<script setup lang="ts">
import { ref } from "vue"
import ChildComp from "../components/child-comp.vue"
const a = ref("")
const b = ref("")
</script>

<template>
  <ChildComp>
    <button @click="a = ''; b = ''">reset</button>
  </ChildComp>
</template>
