<script setup>
import ChildComp from "./child-comp.vue"

function onSelect() {}
</script>

<template>
  <ChildComp label="Pick" :count="3" @select="onSelect" />
</template>
