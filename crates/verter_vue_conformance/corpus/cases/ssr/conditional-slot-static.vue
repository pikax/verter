<script setup lang="ts">
import ChildComp from "../components/child-comp.vue"
</script>

<template>
  <ChildComp>
    <template v-if="true" #actions>x</template>
  </ChildComp>
</template>
