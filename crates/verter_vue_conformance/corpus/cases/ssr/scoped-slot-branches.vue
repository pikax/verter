<script setup lang="ts">
import ChildComp from "../components/child-comp.vue"
</script>

<template>
  <ChildComp>
    <template #cell="{ row }">
      <span v-if="row.a">a</span>
      <template v-else>{{ row.b }}</template>
    </template>
  </ChildComp>
</template>
