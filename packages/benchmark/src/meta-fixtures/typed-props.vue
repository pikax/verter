<script setup lang="ts">
interface ButtonSize {
  width: number
  height: number
}

const props = defineProps<{
  label: string
  disabled: boolean
  size: ButtonSize
  variant: 'primary' | 'secondary' | 'danger'
  icon?: string
}>()

const emit = defineEmits<{
  click: [event: MouseEvent]
  hover: [isHovered: boolean]
}>()
</script>

<template>
  <button
    :class="[variant, { disabled }]"
    :style="{ width: size.width + 'px', height: size.height + 'px' }"
    @click="emit('click', $event)"
    @mouseenter="emit('hover', true)"
    @mouseleave="emit('hover', false)"
  >
    <span v-if="icon" class="icon">{{ icon }}</span>
    {{ label }}
  </button>
</template>
