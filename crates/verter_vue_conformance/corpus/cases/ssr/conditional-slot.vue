<script setup lang="ts">
import ChildComp from "../components/child-comp.vue"
const show = true
</script>

<template>
  <ChildComp>
    <template v-if="show" #actions>x</template>
  </ChildComp>
</template>
