<script setup lang="ts">
import { ref, computed } from 'vue'

const props = defineProps<{
  label: string
  max: number
}>()

const modelValue = defineModel<string>({ required: true })
const checked = defineModel<boolean>('checked', { default: false })

defineSlots<{
  prefix(props: { value: string }): any
}>()

const inputRef = ref<HTMLInputElement | null>(null)
const isEmpty = computed(() => modelValue.value.length === 0)

function focus() {
  inputRef.value?.focus()
}

function clear() {
  modelValue.value = ''
}

function validate(): boolean {
  return modelValue.value.length <= props.max
}

defineExpose({ focus, clear, validate })
</script>

<template>
  <div>
    <label>{{ label }}</label>
    <slot name="prefix" :value="modelValue" />
    <input
      ref="inputRef"
      v-model="modelValue"
      :maxlength="max"
    />
    <input type="checkbox" v-model="checked" />
    <span v-if="isEmpty">Empty</span>
  </div>
</template>
