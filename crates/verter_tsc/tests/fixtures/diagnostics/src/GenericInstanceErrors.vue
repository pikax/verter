<script setup lang="ts">
import GenericComp from './GenericComp.vue'

const foo = {} as InstanceType<typeof GenericComp<"myName">>
const name: number = foo.$props.name  // TS2322: 'myName' not assignable to number
</script>
<template><div /></template>
