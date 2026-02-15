<script setup lang="ts">
import { ref } from 'vue'

const regularCount = ref(0)
const onceCount = ref(0)
const parentCount = ref(0)
const enterCount = ref(0)

function regularClick() { regularCount.value++ }
function onceClick() { onceCount.value++ }
function parentClick() { parentCount.value++ }
function enterHandler() { enterCount.value++ }
</script>

<template>
  <div data-testid="event-modifiers">
    <button data-testid="regular-btn" @click="regularClick">Regular</button>
    <span data-testid="regular-count">{{ regularCount }}</span>

    <button data-testid="once-btn" @click.once="onceClick">Once</button>
    <span data-testid="once-count">{{ onceCount }}</span>

    <div data-testid="parent-area" @click="parentClick">
      <button data-testid="stop-btn" @click.stop="() => {}">Stop Propagation</button>
      <span data-testid="parent-count">{{ parentCount }}</span>
    </div>

    <input data-testid="enter-input" @keyup.enter="enterHandler" />
    <span data-testid="enter-count">{{ enterCount }}</span>
  </div>
</template>
