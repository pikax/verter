<script setup>
defineProps({
  label: { type: String, required: true },
  count: { type: Number, default: 0 },
})
const emit = defineEmits(["select"])

function onClick() {
  emit("select")
}
</script>

<template>
  <button class="child-comp" @click="onClick">
    <slot name="header"></slot>
    {{ label }} ({{ count }})
    <slot></slot>
    <slot name="footer" :total="count"></slot>
  </button>
</template>
