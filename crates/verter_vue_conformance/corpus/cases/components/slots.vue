<script setup>
import ChildComp from "./child-comp.vue"
</script>

<template>
  <ChildComp label="Slotted">
    <template #header>
      <h1>Title</h1>
    </template>
    <template #default="{ rows }">
      <p>{{ rows.length }} rows</p>
    </template>
    <template #footer="{ total }">
      <span>Total {{ total }}</span>
    </template>
  </ChildComp>
</template>
