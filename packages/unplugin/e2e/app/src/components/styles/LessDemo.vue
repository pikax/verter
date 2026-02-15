<script setup lang="ts">
import { ref } from 'vue'
const variant = ref<'primary' | 'secondary'>('primary')

function toggleVariant() {
  variant.value = variant.value === 'primary' ? 'secondary' : 'primary'
}
</script>

<template>
  <div data-testid="less-demo">
    <p data-testid="less-text" :class="variant">Less Styled Text</p>
    <button data-testid="less-toggle" @click="toggleVariant">Toggle Variant</button>
  </div>
</template>

<style lang="less" scoped>
@primary-color: rgb(0, 128, 0);
@secondary-color: rgb(200, 100, 0);

.primary {
  color: @primary-color;
}

.secondary {
  color: @secondary-color;
}
</style>
