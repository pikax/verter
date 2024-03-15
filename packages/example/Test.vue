<script setup lang="ts">
import Dep from './Dep.vue'
defineProps<{
  foo: string,
  bar: number
}>()
</script>
<template>
<div>
    <!-- this is a comment :) 
    
        Thanks x
    -->
    <div></div>
    <!-- @ts-expect-error -->
    <div @click="(test) => test.xx"></div> 

    <Dep/>
  </div> 
</template> 
  