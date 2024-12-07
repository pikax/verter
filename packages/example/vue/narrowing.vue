<script setup>
import { ref } from 'vue'

const disableDrag = ref(false)
</script>

<template>
  <div v-if="disableDrag">
    {{ disableDrag }}
    <slot />
  </div>
  <div v-else-if="!disableDrag">
    {{ disableDrag }}
    <slot />
  </div>
  <div v-else>
    {{ disableDrag }}
    <slot />
  </div>
</template>