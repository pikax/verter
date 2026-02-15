<script setup lang="ts">
import { ref } from 'vue'
import DefineModelDemo from './DefineModelDemo.vue'

const parentValue = ref('initial')

function reset() {
  parentValue.value = ''
}
</script>

<template>
  <div data-testid="define-model-parent">
    <span data-testid="parent-value">{{ parentValue }}</span>
    <button data-testid="model-reset" @click="reset">Reset</button>
    <DefineModelDemo v-model="parentValue" />
  </div>
</template>
