<script setup lang="ts">
interface Props {
  /**
   * The color of the component
   * @defaultValue 'primary'
   */
  color?: 'error' | 'primary' | 'secondary' | 'success'
  /**
   * The size of the component
   */
  size?: 'sm' | 'md' | 'lg'
  label?: string
  disabled?: boolean
}

defineProps<Props>()
</script>

<template><div /></template>
