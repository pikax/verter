<script setup lang="ts">
import { ref } from 'vue'

interface Props {
  label?: string
  count?: number
}

const props = withDefaults(defineProps<Props>(), {
  label: 'default-label',
  count: 42,
})

const overridden = ref(false)

function toggleOverride() {
  overridden.value = !overridden.value
}
</script>

<template>
  <div data-testid="with-defaults">
    <span data-testid="defaults-label">{{ props.label }}</span>
    <span data-testid="defaults-count">{{ props.count }}</span>
    <button data-testid="toggle-override" @click="toggleOverride">Toggle</button>
    <span data-testid="override-state">{{ overridden ? 'overridden' : 'default' }}</span>
  </div>
</template>
