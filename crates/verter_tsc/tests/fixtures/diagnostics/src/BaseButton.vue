<script setup lang="ts">
defineProps<{
  label: string
  disabled?: boolean
  size: 'sm' | 'md' | 'lg'
  count: number
  variant?: 'primary' | 'secondary'
}>()

const emit = defineEmits<{
  click: [event: MouseEvent]
  update: [value: string]
  reset: []
}>()

const model = defineModel<string>()

defineSlots<{
  default(props: { active: boolean }): any
  icon(): any
}>()

function handleClick(e: MouseEvent) {
  emit('click', e)
}
</script>
<template>
  <button :disabled="disabled" @click="handleClick">
    <slot name="icon" />
    <slot :active="!disabled" />
    {{ label }} ({{ count }}) {{ model }}
  </button>
</template>
