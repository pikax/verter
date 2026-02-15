<script lang="ts">
import { defineComponent } from 'vue'

export default defineComponent({
  props: {
    label: { type: String, required: true },
  },
  emits: ['action'],
  methods: {
    doAction() {
      this.$emit('action', 'options-action')
    },
  },
})
</script>

<template>
  <div data-testid="options-component">
    <span data-testid="options-label">{{ label }}</span>
    <button data-testid="options-action" @click="doAction">Action</button>
  </div>
</template>
