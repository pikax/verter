<script setup lang="ts">
import TypedSlotComp from './TypedSlotComp.vue'

const outerLabel = 'outer'
</script>

<template>
  <TypedSlotComp v-slot="{ slotItem, slotIndex, slotTotal }">
    <p>{{ slotItem.name }}</p>
    <p>{{ slotIndex }}</p>
    <p>{{ slotTotal }}</p>
    <p>{{ outerLabel }}</p>
  </TypedSlotComp>
</template>
