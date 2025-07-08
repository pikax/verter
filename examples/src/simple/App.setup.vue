<script lang="ts" setup>
defineProps<{
  name: string;
}>();

// new ___VERTER___Component().$props.name
</script>
<template>
  <div>
    <span>{{ name }}</span>
  </div>    
</template>