<script setup lang="ts">
import { ref } from 'vue'
import { MyComp } from '@/components'
const count = ref(0)
</script>
<template>
  <div>{{ count }}</div>
  <MyComp :foo="'a'" :bar="1" />
</template>
