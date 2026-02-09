<template>
  <MyComponent>
    <template #header="{ title }">
      {{ title }}
    </template>
  </MyComponent>
</template>

<script setup>
import MyComponent from './MyComponent.vue'
</script>
